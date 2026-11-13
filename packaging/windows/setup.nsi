; Inkdown guided Windows installer.
;
; Built by .github/workflows/build.yml with:
;   makensis /DVERSION=0.1.5 /DROOT=<repo> /DOUTFILE=<abs path> setup.nsi
;
; It installs per-user (no UAC), lets the user pick the install, data and
; config folders, optionally creates a desktop shortcut, and writes
; install.json next to the executable so the app knows where config/data live.

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user

!ifndef VERSION
  !define VERSION "0.1.5"
!endif
!ifndef ROOT
  !define ROOT "..\.."
!endif
!ifndef OUTFILE
  !define OUTFILE "Inkdown-Setup.exe"
!endif

Name "Inkdown"
Caption "Inkdown 安装程序"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\Inkdown"
InstallDirRegKey HKCU "Software\Inkdown" "InstallDir"
BrandingText "Inkdown · Markdown 写作与分发"
ShowInstDetails show
ShowUninstDetails show
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "Inkdown"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "CompanyName" "ice-a"
VIAddVersionKey "FileDescription" "Inkdown 安装程序"
VIAddVersionKey "LegalCopyright" "MIT"

!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "LogicLib.nsh"
!include "StrFunc.nsh"
!include "FileFunc.nsh"

; Generate the StrRep helper used to write JSON-safe paths.
${StrRep}

!define MUI_ABORTWARNING
!define MUI_ICON "${ROOT}\assets\icon\inkdown.ico"
!define MUI_UNICON "${ROOT}\assets\icon\inkdown.ico"
!define MUI_WELCOMEPAGE_TITLE "安装 Inkdown"
!define MUI_WELCOMEPAGE_TEXT "Inkdown 是一款 Markdown 写作与分发工具。$\r$\n$\r$\n安装过程不需要管理员权限，程序会装到当前用户目录，并保留你自己的文稿、配置与备份。"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
Page custom DataDirPageCreate DataDirPageLeave
Page custom ConfigDirPageCreate ConfigDirPageLeave
!insertmacro MUI_PAGE_INSTFILES

!define MUI_FINISHPAGE_RUN "$INSTDIR\inkdown.exe"
!define MUI_FINISHPAGE_RUN_TEXT "启动 Inkdown"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

Var DataDir
Var DataDirEdit
Var ConfigDir
Var ConfigDirEdit
Var DesktopShortcut
Var DesktopCheck

Function .onInit
  ; Upgrades keep the previous choices; fresh installs get per-user defaults.
  ReadRegStr $DataDir HKCU "Software\Inkdown" "DataDir"
  ${If} $DataDir == ""
    StrCpy $DataDir "$LOCALAPPDATA\Inkdown\data"
  ${EndIf}
  ReadRegStr $ConfigDir HKCU "Software\Inkdown" "ConfigDir"
  ${If} $ConfigDir == ""
    StrCpy $ConfigDir "$LOCALAPPDATA\Inkdown\config"
  ${EndIf}
  StrCpy $DesktopShortcut "1"
FunctionEnd

Function DataDirPageCreate
  !insertmacro MUI_HEADER_TEXT "数据存储位置" "备份和没有归属文稿的图片会放在这里。"
  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}

  ${NSD_CreateLabel} 0 0 100% 30u "选择数据目录。Inkdown 的 zip 备份会写入其中的 backups 文件夹；已有文件不会被删除。"
  Pop $0

  ${NSD_CreateText} 0 34u -82u 14u "$DataDir"
  Pop $DataDirEdit
  ${NSD_CreateButton} -76u 33u 76u 16u "浏览..."
  Pop $0
  ${NSD_OnClick} $0 DataDirBrowse

  ${NSD_CreateLabel} 0 58u 100% 36u "默认位置：%LOCALAPPDATA%\Inkdown\data。如果使用便携版或想放到网盘/移动硬盘，可以改成任意目录。"
  Pop $0

  nsDialogs::Show
FunctionEnd

Function DataDirBrowse
  ${NSD_GetText} $DataDirEdit $0
  nsDialogs::SelectFolderDialog "选择数据存储位置" "$0"
  Pop $1
  ${If} $1 != ""
    ${NSD_SetText} $DataDirEdit "$1"
  ${EndIf}
FunctionEnd

Function DataDirPageLeave
  ${NSD_GetText} $DataDirEdit $DataDir
  ${If} $DataDir == ""
    MessageBox MB_ICONEXCLAMATION|MB_OK "请选择数据存储位置。"
    Abort
  ${EndIf}
FunctionEnd

Function ConfigDirPageCreate
  !insertmacro MUI_HEADER_TEXT "配置存储位置" "config.json 会放在这里，之后可以单独备份或导入。"
  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}

  ${NSD_CreateLabel} 0 0 100% 30u "选择配置目录。图床、公众号、博客园、Git 与 AI 的设置都会写入其中的 config.json。"
  Pop $0

  ${NSD_CreateText} 0 34u -82u 14u "$ConfigDir"
  Pop $ConfigDirEdit
  ${NSD_CreateButton} -76u 33u 76u 16u "浏览..."
  Pop $0
  ${NSD_OnClick} $0 ConfigDirBrowse

  ${NSD_CreateLabel} 0 58u 100% 34u "默认位置：%LOCALAPPDATA%\Inkdown\config。密钥以明文保存在该文件中，请勿提交到公开仓库。"
  Pop $0

  ${NSD_CreateCheckbox} 0 100u 100% 12u "在桌面创建 Inkdown 快捷方式"
  Pop $DesktopCheck
  ${NSD_Check} $DesktopCheck

  nsDialogs::Show
FunctionEnd

Function ConfigDirBrowse
  ${NSD_GetText} $ConfigDirEdit $0
  nsDialogs::SelectFolderDialog "选择配置存储位置" "$0"
  Pop $1
  ${If} $1 != ""
    ${NSD_SetText} $ConfigDirEdit "$1"
  ${EndIf}
FunctionEnd

Function ConfigDirPageLeave
  ${NSD_GetText} $ConfigDirEdit $ConfigDir
  ${If} $ConfigDir == ""
    MessageBox MB_ICONEXCLAMATION|MB_OK "请选择配置存储位置。"
    Abort
  ${EndIf}
  ${NSD_GetState} $DesktopCheck $0
  ${If} $0 = 1
    StrCpy $DesktopShortcut "1"
  ${Else}
    StrCpy $DesktopShortcut "0"
  ${EndIf}
FunctionEnd

Section "Inkdown" SecMain
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File "/oname=inkdown.exe" "${ROOT}\inkdown.exe"

  CreateDirectory "$DataDir"
  CreateDirectory "$ConfigDir"

  ; install.json is the app's bootstrap file: it points config.json and
  ; generated data at the folders chosen above, and survives upgrades.
  ${StrRep} $R0 "$DataDir" "\" "/"
  ${StrRep} $R1 "$ConfigDir" "\" "/"
  ClearErrors
  FileOpen $0 "$INSTDIR\install.json" w
  IfErrors install_json_failed
  FileWrite $0 '{"version":"${VERSION}",$\r$\n'
  FileWrite $0 '"data_dir":"$R0",$\r$\n'
  FileWrite $0 '"config_dir":"$R1"}$\r$\n'
  FileClose $0
  Goto install_json_done
install_json_failed:
  DetailPrint "警告：无法写入 install.json，将使用默认配置目录。"
install_json_done:

  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateDirectory "$SMPROGRAMS\Inkdown"
  CreateShortcut "$SMPROGRAMS\Inkdown\Inkdown.lnk" "$INSTDIR\inkdown.exe" "" "$INSTDIR\inkdown.exe" 0
  CreateShortcut "$SMPROGRAMS\Inkdown\卸载 Inkdown.lnk" "$INSTDIR\Uninstall.exe"
  ${If} $DesktopShortcut == "1"
    CreateShortcut "$DESKTOP\Inkdown.lnk" "$INSTDIR\inkdown.exe" "" "$INSTDIR\inkdown.exe" 0
  ${EndIf}

  WriteRegStr HKCU "Software\Inkdown" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Inkdown" "DataDir" "$DataDir"
  WriteRegStr HKCU "Software\Inkdown" "ConfigDir" "$ConfigDir"

  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Inkdown" "DisplayName" "Inkdown"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Inkdown" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Inkdown" "Publisher" "ice-a"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Inkdown" "DisplayIcon" "$INSTDIR\inkdown.exe,0"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Inkdown" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Inkdown" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Inkdown" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Inkdown" "NoRepair" 1

  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Inkdown" "EstimatedSize" "$0"
SectionEnd

Section "Uninstall"
  SetShellVarContext current
  Delete "$DESKTOP\Inkdown.lnk"
  Delete "$SMPROGRAMS\Inkdown\Inkdown.lnk"
  Delete "$SMPROGRAMS\Inkdown\卸载 Inkdown.lnk"
  RMDir "$SMPROGRAMS\Inkdown"

  Delete "$INSTDIR\inkdown.exe"
  Delete "$INSTDIR\install.json"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"

  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Inkdown"
  DeleteRegKey HKCU "Software\Inkdown"
  ; Data and config folders are intentionally left untouched.
SectionEnd
