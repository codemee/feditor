// @name 待辦清單（Markdown）
// @extensions todo
// @output markdown
//
// 一行一個待辦事項：「x 」開頭是已完成，其他是未完成；「# 」開頭的行是標題。
function preview(text, info) {
  const out = [];
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line) continue;
    if (line.startsWith("# ")) { out.push("\n" + line + "\n"); continue; }
    out.push(line.startsWith("x ") ? "- [x] " + line.slice(2) : "- [ ] " + line);
  }
  return out.join("\n");
}
