// 把全文轉成大寫
const text = editor.text();
editor.replace(0, editor.length(), text.toUpperCase());
console.log("已轉大寫，共 " + editor.length() + " 個字元");
