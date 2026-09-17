Set shell = CreateObject("WScript.Shell")
repo = "D:\GanMaoYuan\Website-Clone"
cmd = "powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File """ & repo & "\scripts\start-ganmaoyuan-desktop.ps1"""
shell.CurrentDirectory = repo
shell.Run cmd, 0, False
