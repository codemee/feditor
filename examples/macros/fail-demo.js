// 故意失敗的巨集：先修改，再丟出錯誤。用來驗證「失敗時還原所有修改」。
editor.insert(0, "這行不應該留下來\n");
console.log("已插入一行，接著會失敗…");
throw new Error("故意的錯誤");
