// 在每一行開頭加上行號（由後往前插入，這樣前面的位置不會因插入而位移）
const n = editor.lineCount();
let offsets = [];
let pos = 0;
for (let i = 0; i < n; i++) {
  offsets.push(pos);
  const line = editor.line(i);
  pos += Array.from(line).length; // 以碼點計算，與 editor 的位置單位一致
}
for (let i = n - 1; i >= 0; i--) {
  // 最後一個空行（檔尾換行之後）不加行號
  if (i === n - 1 && editor.line(i) === "") continue;
  editor.insert(offsets[i], (i + 1) + ": ");
}
console.log("已為 " + n + " 行加上行號");
