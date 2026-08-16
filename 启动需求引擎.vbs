Option Explicit
' Silent desktop launcher. WindowStyle 0 = no console.
Dim fso, sh, root, engine, exe, dataDir, cmd
Set fso = CreateObject("Scripting.FileSystemObject")
Set sh = CreateObject("WScript.Shell")
root = fso.GetParentFolderName(WScript.ScriptFullName)
engine = fso.BuildPath(root, "req-engine")

If Not fso.FolderExists(engine) Then
  MsgBox "cannot find req-engine folder", 16, "Req-Engine"
  WScript.Quit 1
End If

exe = ""
If fso.FileExists(fso.BuildPath(engine, "target\release\req-engine.exe")) Then
  exe = fso.BuildPath(engine, "target\release\req-engine.exe")
ElseIf fso.FileExists(fso.BuildPath(engine, "target\debug\req-engine.exe")) Then
  exe = fso.BuildPath(engine, "target\debug\req-engine.exe")
End If

If exe = "" Then
  MsgBox "No req-engine.exe found." & vbCrLf & _
    "Open a terminal:" & vbCrLf & _
    "  cd req-engine" & vbCrLf & _
    "  cargo build --release", 16, "Req-Engine"
  WScript.Quit 1
End If

dataDir = fso.BuildPath(engine, "data")
If Not fso.FolderExists(dataDir) Then fso.CreateFolder dataDir

sh.CurrentDirectory = engine
sh.Environment("Process")("REQ_ENGINE_SILENT") = "1"
cmd = """" & exe & """ desktop --home """ & dataDir & """ --port 7420"
sh.Run cmd, 0, False
