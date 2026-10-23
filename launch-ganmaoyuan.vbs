Set shell = CreateObject("WScript.Shell")
Set fso = CreateObject("Scripting.FileSystemObject")
repo = fso.GetParentFolderName(WScript.ScriptFullName)
cmd = "powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File """ & repo & "\scripts\start-ganmaoyuan-desktop.ps1"""
shell.CurrentDirectory = repo
shell.Run cmd, 0, False
