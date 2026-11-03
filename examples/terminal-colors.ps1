# 終端機顏色與屬性測試：在 FEditor 的終端機窗格裡執行這個檔案。
# Terminal color and attribute test: run this file inside the FEditor terminal pane.
#   pwsh -File examples\terminal-colors.ps1 [-Only 1,2]

param([string[]]$Only)   # 只跑指定的段落，例如 -Only 1,4；不指定就全部 / run only these sections

$e = [char]27
$reset = "$e[0m"

function Show-Title($text) { Write-Host ""; Write-Host "$e[1m$text$reset" }
# -File 模式下 1,2,3 會被當成一個字串，所以自己用逗號拆開 / with -File, "1,2,3" arrives as one string, so split it here
$selected = @($Only | ForEach-Object { $_ -split '[,\s]+' } | Where-Object { $_ } | ForEach-Object { [int]$_ })
function Want($n) { -not $selected -or $selected -contains $n }

if (Want 1) {
  Show-Title '1. 16 色前景（30-37、90-97）／16 foreground colors'
  $line = ''
  foreach ($c in 30..37 + 90..97) { $line += "$e[${c}m{0,4}$reset" -f $c }
  Write-Host $line
}

if (Want 2) {
  Show-Title '2. 16 色背景（40-47、100-107）／16 background colors'
  $line = ''
  foreach ($c in 40..47 + 100..107) { $line += "$e[${c}m{0,4}$reset" -f $c }
  Write-Host $line
}

if (Want 3) {
  Show-Title '3. 粗體 + 16 色／bold + 16 colors'
  $line = ''
  foreach ($c in 30..37) { $line += "$e[1;${c}m{0,4}$reset" -f $c }
  Write-Host $line
}

if (Want 4) {
  Show-Title '4. 256 色：0-15 系統色、16-231 色立方、232-255 灰階／256-color palette'
  $line = ''
  foreach ($i in 0..15) { $line += "$e[48;5;${i}m  $reset" }
  Write-Host $line
  foreach ($row in 0..5) {
    $line = ''
    foreach ($i in (16 + $row * 36)..(16 + $row * 36 + 35)) { $line += "$e[48;5;${i}m  $reset" }
    Write-Host $line
  }
  $line = ''
  foreach ($i in 232..255) { $line += "$e[48;5;${i}m  $reset" }
  Write-Host $line
}

if (Want 5) {
  Show-Title '5. 真彩色漸層（紅→綠→藍）／truecolor gradient'
  $line = ''
  foreach ($i in 0..63) {
    $r = [int](255 * [Math]::Max(0, 1 - $i / 32))
    $g = [int](255 * (1 - [Math]::Abs($i - 32) / 32))
    $b = [int](255 * [Math]::Max(0, ($i - 32) / 32))
    $line += "$e[48;2;${r};${g};${b}m $reset"
  }
  Write-Host $line
  $line = ''
  foreach ($i in 0..63) { $v = $i * 4; $line += "$e[38;2;${v};${v};255mx$reset" }
  Write-Host $line
}

if (Want 6) {
  Show-Title '6. 文字屬性／attributes'
  Write-Host "$e[1m粗體 bold$reset  $e[2m變暗 dim$reset  $e[3m斜體 italic$reset  $e[4m底線 underline$reset  $e[9m刪除線 strike$reset  $e[7m反相 inverse$reset  $e[8m隱藏 hidden$reset(隱藏的字看不到)"
  Write-Host "$e[31;47m紅字白底$reset  $e[97;41m亮白字紅底$reset  $e[7;32m反相的綠$reset  $e[1;4;33m粗體底線黃$reset"
}

if (Want 7) {
  Show-Title '7. 預設前景／背景（沒設顏色的字）／default colors'
  Write-Host "這行沒有設任何顏色 This line has no color set"
  Write-Host "$e[39;49m39/49 還原預設色 reset to default$reset"
}

if (Want 8) {
  Show-Title '8. 全形字與表格線／wide characters and box drawing'
  Write-Host "中文 日本語 한국어 ＡＢＣ１２３ 😀"
  Write-Host "┌──┬──┐"
  Write-Host "│你│好│"
  Write-Host "└──┴──┘"
}

if (Want 9) {
  Show-Title '9. PowerShell 內建色名／PowerShell color names'
  foreach ($name in [Enum]::GetNames([System.ConsoleColor])) {
    Write-Host ("{0,-12}" -f $name) -ForegroundColor $name -NoNewline
    Write-Host ("{0,-12}" -f $name) -BackgroundColor $name -ForegroundColor White
  }
  Write-Host ""
}

if (Want 10) {
  Show-Title '10. 低對比度（最低對比度保護會把看不清楚的字調到看得清楚）／low contrast (the minimum-contrast guard makes these readable)'
  Write-Host "$e[30;40m黑字黑底 black on black$reset  $e[37;47m白字白底 white on white$reset  $e[97;107m亮白字亮白底$reset  $e[33;47m黃字白底 yellow on white$reset"
  Write-Host "$e[38;2;40;40;40m深灰字（真彩色）dark gray text$reset  $e[38;2;235;235;235m淺灰字（真彩色）light gray text$reset  $e[90;100m灰字灰底$reset"
  Write-Host "進度條與表格線不受影響 graphic glyphs are left alone: $e[34;44m████$reset$e[36;46m▓▓▓▓$reset ┌─┬─┐"
}