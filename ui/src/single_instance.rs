//! 單一執行個體：用命令列給路徑啟動（檔案總管、Finder 的右鍵選單）時，如果已經有 FEditor 在執行，
//! 就把路徑交給它去開，自己直接結束；沒有才自己啟動。
//!
//! 做法：第一個啟動的 FEditor 在本機的固定埠（127.0.0.1）上監聽。埠只能被一個行程綁住，
//! 所以「綁得到」就是「我是第一個」，這是不會有時間差的鎖；行程結束（包括當掉）時系統自動放掉，不會有殘留的鎖檔。
//! 後來啟動的綁不到，就連上去把路徑傳過去。
//!
//! 通訊協定（一行一筆文字）：
//! ```text
//! 用戶端 → FEDITOR2 <使用者名稱>
//! 用戶端 → wait（要等檔案關閉才結束時才有這一行）
//! 用戶端 → open<TAB><行><TAB><欄><TAB><絕對路徑>（有幾個就幾行；行、欄是 0 表示沒指定）
//! 用戶端 → （空行，結束）
//! 伺服器 → OK
//! 伺服器 → DONE（只有 wait 才有：那些檔案都關閉之後）
//! ```
//! 第一行必須是這個固定的開頭加上同一個使用者名稱：網頁或別的程式亂送的資料（例如 HTTP 請求）
//! 開頭對不上就直接丟掉；同一台機器上別的使用者的 FEditor 也不會互相收到路徑。

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use crate::cli::Target;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

/// 監聽的埠。被別的軟體佔走時，FEditor 會當成「連不上」，照常自己啟動（只是不會互相轉交）。
pub const PORT: u16 = 49731;

const MAGIC: &str = "FEDITOR2";

/// 轉交來的一批要開的檔案。
pub struct Request {
    pub targets: Vec<Target>,
    /// 對方在等：這些檔案都關閉之後，對 `done` 送一個訊號（或直接丟掉它，表示不用等了）。
    pub done: Option<mpsc::Sender<()>>,
}

/// 啟動時的角色。
pub enum Claim {
    /// 已經有 FEditor 在執行，路徑已經交給它了：這個行程可以結束。
    Forwarded,
    /// 我是第一個：用 [`serve`] 開始接收別人轉交的路徑。
    Primary(TcpListener),
    /// 沒辦法協調（埠被別的軟體佔用、連不上、對方不是 FEditor…）：照常自己啟動。
    Standalone,
}

fn user_name() -> String {
    std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default().replace(['\r', '\n'], " ")
}

fn address(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

/// 決定這個行程的角色。沒有檔案的啟動（例如從開始功能表）一律開新視窗，不會去驚動別的視窗；
/// 有檔案時，已經有 FEditor 在跑就轉交給它（`wait` 時等到那些檔案關閉才回來）；`new_window` 則不轉交。
pub fn claim(targets: &[Target], wait: bool, new_window: bool) -> Claim {
    claim_at(PORT, targets, wait, new_window)
}

pub fn claim_at(port: u16, targets: &[Target], wait: bool, new_window: bool) -> Claim {
    let addr = address(port);
    if new_window {
        return TcpListener::bind(addr).map_or(Claim::Standalone, Claim::Primary);
    }
    // 同時啟動好幾個（在檔案總管選了很多個檔案）時，第一個綁到的是主角，其餘的稍等它開始接收再轉交
    for attempt in 0..10 {
        match TcpListener::bind(addr) {
            Ok(listener) => return Claim::Primary(listener),
            Err(_) if targets.is_empty() => return Claim::Standalone,
            Err(_) => {
                if forward(addr, targets, wait) {
                    return Claim::Forwarded;
                }
                if attempt < 9 {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
    Claim::Standalone
}

/// 把檔案交給 `addr` 上的 FEditor。對方確認收到才回傳 `true`；`wait` 時還會等到對方說那些檔案關閉了。
fn forward(addr: SocketAddr, targets: &[Target], wait: bool) -> bool {
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    allow_foreground();
    let mut message = format!("{MAGIC} {}\n", user_name());
    if wait {
        message.push_str("wait\n");
    }
    for target in targets {
        // 相對路徑要用「這個行程」的目前資料夾換成絕對路徑，對方的目前資料夾不一樣
        let absolute = std::path::absolute(&target.path).unwrap_or_else(|_| target.path.clone());
        if let Some(text) = absolute.to_str().filter(|t| !t.contains(['\r', '\n'])) {
            message.push_str(&format!("open\t{}\t{}\t{text}\n", target.line.unwrap_or(0), target.col.unwrap_or(0)));
        }
    }
    message.push('\n');
    if stream.write_all(message.as_bytes()).is_err() {
        return false;
    }
    let mut reader = BufReader::new(stream);
    let mut reply = String::new();
    if reader.read_line(&mut reply).is_err() || reply.trim() != "OK" {
        return false;
    }
    if wait {
        // 檔案關閉要多久不一定：不設逾時。對方結束（連線斷了）也就不用等了
        let _ = reader.get_ref().set_read_timeout(None);
        let mut done = String::new();
        let _ = reader.read_line(&mut done);
    }
    true
}

/// Windows 不讓背景行程把視窗拉到最前面；剛被使用者操作（右鍵選單）啟動的這個行程可以授權對方。
fn allow_foreground() {
    #[cfg(windows)]
    // SAFETY: 單純的系統呼叫，沒有指標參數；ASFW_ANY 是 u32::MAX
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(u32::MAX);
    }
}

/// 開始接收別人轉交的路徑：背景執行緒處理連線，每收到一批就呼叫 `notify`（用來喚醒畫面）並把路徑送進回傳的通道。
pub fn serve(listener: TcpListener, notify: impl Fn() + Send + Sync + 'static) -> mpsc::Receiver<Request> {
    let (tx, rx) = mpsc::channel();
    let notify = std::sync::Arc::new(notify);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            // 每個連線各用一個執行緒：有人連上來卻不說話，不會卡住後面正常的請求
            let (tx, notify) = (tx.clone(), notify.clone());
            std::thread::spawn(move || {
                let Some((targets, wait, mut stream)) = read_request(stream) else { return };
                if targets.is_empty() {
                    return;
                }
                let (done_tx, done_rx) = mpsc::channel();
                let done = wait.then_some(done_tx);
                if tx.send(Request { targets, done }).is_err() {
                    return;
                }
                notify();
                // 等的話就停在這裡，直到畫面那邊說檔案都關了（recv 失敗是畫面結束了，不用再等）
                if wait && done_rx.recv().is_ok() {
                    let _ = stream.write_all(b"DONE\n");
                }
            });
        }
    });
    rx
}

/// 讀一個連線的請求；格式不對就 `None`。正確的話回 `OK`，並把連線交還（`wait` 時之後還要回 `DONE`）。
fn read_request(stream: TcpStream) -> Option<(Vec<Target>, bool, TcpStream)> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut first = String::new();
    // 限制長度：不是我們的用戶端就不要一直讀下去
    std::io::Read::take(&mut reader, 4096).read_line(&mut first).ok()?;
    if first.trim_end() != format!("{MAGIC} {}", user_name()) {
        return None;
    }
    let mut targets = Vec::new();
    let mut wait = false;
    for _ in 0..1000 {
        let mut line = String::new();
        if std::io::Read::take(&mut reader, 32768).read_line(&mut line).ok()? == 0 {
            return None; // 沒收到結束的空行就斷線
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            let mut out = stream;
            out.write_all(b"OK\n").ok()?;
            return Some((targets, wait, out));
        }
        if line == "wait" {
            wait = true;
        } else if let Some(rest) = line.strip_prefix("open\t") {
            let mut f = rest.splitn(3, '\t');
            let (l, c, path) = (f.next()?, f.next()?, f.next()?);
            let num = |t: &str| t.parse::<usize>().ok().filter(|&n| n > 0);
            targets.push(Target { path: PathBuf::from(path), line: num(l), col: num(c) });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 找一個沒人用的埠（綁 0 讓系統挑，再放掉）。
    fn free_port() -> u16 {
        TcpListener::bind(address(0)).unwrap().local_addr().unwrap().port()
    }

    #[test]
    fn the_first_instance_is_primary_and_later_ones_forward_their_paths() {
        let port = free_port();
        let Claim::Primary(listener) = claim_at(port, &[], false, false) else { panic!("第一個應該是主角") };
        let woken = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = woken.clone();
        let rx = serve(listener, move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        // 沒有路徑的啟動（開始功能表）：不轉交，自己開新視窗
        assert!(matches!(claim_at(port, &[], false, false), Claim::Standalone));
        // 有路徑：轉交給主角，自己結束
        let file = std::env::temp_dir().join("feditor single instance 測試.txt");
        let folder = std::env::temp_dir();
        let targets = [Target::plain(file.clone()), Target { path: folder.clone(), line: Some(12), col: Some(3) }];
        assert!(matches!(claim_at(port, &targets, false, false), Claim::Forwarded));
        let got = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(got.targets, targets.to_vec(), "路徑原樣送到（含空白與中文）與行、欄");
        assert!(got.done.is_none());
        // 路徑先進通道、之後才喚醒畫面（畫面醒來一定拿得到路徑），所以收到路徑時喚醒可能還沒執行：稍等一下
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while woken.load(std::sync::atomic::Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(woken.load(std::sync::atomic::Ordering::SeqCst), 1, "每一批喚醒畫面一次");
    }

    #[test]
    fn relative_paths_become_absolute_for_the_receiver() {
        let port = free_port();
        let Claim::Primary(listener) = claim_at(port, &[], false, false) else { panic!() };
        let rx = serve(listener, || {});
        assert!(forward(address(port), &[Target::plain("relative.txt")], false));
        let got = rx.recv_timeout(Duration::from_secs(3)).unwrap().targets;
        assert!(got[0].path.is_absolute() && got[0].path.ends_with("relative.txt"), "{got:?}");
    }

    #[test]
    fn strangers_and_other_users_are_ignored() {
        let port = free_port();
        let Claim::Primary(listener) = claim_at(port, &[], false, false) else { panic!() };
        let rx = serve(listener, || {});
        let send = |text: &str| {
            let mut s = TcpStream::connect(address(port)).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            s.write_all(text.as_bytes()).unwrap();
            let mut reply = String::new();
            let _ = BufReader::new(s).read_line(&mut reply);
            reply
        };
        // 網頁送來的 HTTP 請求、別的使用者、沒有結束空行：都不回 OK，也不開任何檔案
        assert_eq!(send("POST / HTTP/1.1\r\nHost: localhost\r\n\r\n/etc/passwd\n\n"), "");
        assert_eq!(send(&format!("{MAGIC} someone-else\nopen\t0\t0\t/tmp/x\n\n")), "");
        assert_eq!(send(&format!("{MAGIC} {}\nopen\t0\t0\t/tmp/x\n", user_name())), "");
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "不該收到任何路徑");
        // 之後正常的請求還是收得到
        assert!(forward(address(port), &[Target::plain("/tmp/ok.txt")], false));
        assert!(rx.recv_timeout(Duration::from_secs(3)).is_ok());
    }

    #[test]
    fn a_port_taken_by_something_else_falls_back_to_a_normal_start() {
        // 佔住埠但不是 FEditor（接受連線卻不回應）：轉交失敗，重試之後照常啟動
        let blocker = TcpListener::bind(address(0)).unwrap();
        let port = blocker.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for s in blocker.incoming().flatten() {
                drop(s);
            }
        });
        assert!(matches!(claim_at(port, &[Target::plain("x.txt")], false, false), Claim::Standalone));
    }

    #[test]
    fn a_waiting_client_returns_only_after_the_receiver_says_done() {
        let port = free_port();
        let Claim::Primary(listener) = claim_at(port, &[], false, false) else { panic!() };
        let rx = serve(listener, || {});
        let client = std::thread::spawn(move || forward(address(port), &[Target::plain("/tmp/w.txt")], true));
        let req = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(!client.is_finished(), "檔案還沒關，不該回來");
        req.done.unwrap().send(()).unwrap();
        assert!(client.join().unwrap());
    }

    #[test]
    fn a_waiting_client_gives_up_waiting_when_the_receiver_goes_away() {
        let port = free_port();
        let Claim::Primary(listener) = claim_at(port, &[], false, false) else { panic!() };
        let rx = serve(listener, || {});
        let client = std::thread::spawn(move || forward(address(port), &[Target::plain("/tmp/w.txt")], true));
        drop(rx.recv_timeout(Duration::from_secs(3)).unwrap()); // 丟掉 done：不用等了
        assert!(client.join().unwrap());
    }

    #[test]
    fn new_window_never_forwards() {
        let port = free_port();
        let Claim::Primary(listener) = claim_at(port, &[], false, false) else { panic!() };
        let rx = serve(listener, || {});
        assert!(matches!(claim_at(port, &[Target::plain("/tmp/n.txt")], false, true), Claim::Standalone));
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    }
}
