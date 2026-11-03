// @name INI 樹狀檢視
// @extensions ini, cfg, conf
// @output tree
//
// 把 INI 設定檔轉成「區段 → 鍵 = 值」的樹。`;` 與 `#` 開頭的行是註解。
function preview(text, info) {
  const root = {};
  let current = root;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line[0] === ";" || line[0] === "#") continue;
    const section = line.match(/^\[(.+)\]$/);
    if (section) {
      current = root[section[1]] = root[section[1]] || {};
      continue;
    }
    const pair = line.match(/^([^=]+?)\s*=\s*(.*)$/);
    if (pair) current[pair[1]] = pair[2];
  }
  return root;
}
