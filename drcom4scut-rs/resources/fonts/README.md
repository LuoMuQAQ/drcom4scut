# Bundled UI fonts

Noto Sans SC Regular (400) and Medium (500), generated as static TrueType instances from the official Noto CJK variable font. Source URL, pinned revision and SHA-256 values are in sources.json. The copyright, modification notice and SIL OFL 1.1 are in NOTICE.txt. The TrueType instances are used because the CFF/OTF version did not pass the native partial repaint consistency regression on the test machine.

The full upstream SC character coverage is retained, including dynamic adapter names and user text. These are not subsets of the current UI strings. Run `python generate.py /path/to/NotoSansSC-VF.ttf` with fonttools==4.66.0 to reproduce the instances. The script verifies the pinned source SHA-256 and preserves font timestamps. Normal Rust builds require neither Python nor a network connection.

The program decompresses and registers each font once per process using AddFontMemResourceEx before creating any UI font. This does not install fonts on Windows. Private font registrations last for the Windows process lifetime. Main, setup and uninstall all use ui::winutil::create_font. System-managed UAC, MessageBox and shell dialogs retain the OS font.

Font family names: `Noto Sans SC`, `Noto Sans SC Medium`. Registration failure falls back to the existing system font chain. No runtime download or external font file is required. Full notices are embedded in every executable and included in installed licenses/NOTICE.txt.
