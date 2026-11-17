# 第三方元件授權 / Third-party notices

以下是隨附在 Windows 版執行檔裡的第三方元件。
The following third-party components are bundled in the Windows build.

## Windows ConPTY（`conpty.dll` 與 `OpenConsole.exe`）

- **用途 / Purpose**：Windows 內建的 ConPTY 會吃掉程式問終端機的問題（背景色、前景色等），Codex 這類程式因此無法判斷深色或淺色。Windows 版把微軟的新版 ConPTY 內嵌在 `feditor.exe` 裡，第一次開終端機時解開到 `%LOCALAPPDATA%\feditor\conpty\<版本>\` 再載入。 / The ConPTY built into Windows swallows the questions programs ask the terminal (background color, foreground color…), so programs such as Codex cannot tell a dark terminal from a light one. The Windows build embeds Microsoft's newer ConPTY in `feditor.exe` and unpacks it to `%LOCALAPPDATA%\feditor\conpty\<version>\` the first time a terminal is opened.
- **來源 / Source**：NuGet 套件 [Microsoft.Windows.Console.ConPTY](https://www.nuget.org/packages/Microsoft.Windows.Console.ConPTY) 1.25.261109001（Windows Terminal 專案 / the Windows Terminal project，<https://github.com/microsoft/terminal>）。
- **檔案位置 / Files**：`ui/assets/conpty/`。
- **授權 / License**：MIT，著作權屬於 Microsoft Corporation。 / MIT, copyright Microsoft Corporation.

```text
Copyright (c) Microsoft Corporation. All rights reserved.

MIT License

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED *AS IS*, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```