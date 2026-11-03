// @name 記錄檔表格
// @extensions log
// @output table
//
// 把「日期 時間 等級 訊息」格式的記錄檔轉成表格，例如：
//   2026-11-02 12:00:01 INFO 伺服器啟動
// 不符合格式的行，整行放在「訊息」欄。
function preview(text, info) {
  const rows = [];
  for (const line of text.split(/\r?\n/)) {
    if (!line.trim()) continue;
    const m = line.match(/^(\S+\s+\S+)\s+([A-Za-z]+)\s+(.*)$/);
    rows.push(m ? [m[1], m[2], m[3]] : ["", "", line]);
  }
  return { header: ["時間", "等級", "訊息"], rows: rows };
}
