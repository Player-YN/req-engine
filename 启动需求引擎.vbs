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
Dim rel, dbg
rel = fso.BuildPath(engine, "target\release\req-engine.exe")
dbg = fso.BuildPath(engine, "target\debug\req-engine.exe")
If fso.FileExists(rel) And fso.FileExists(dbg) Then
  If fso.GetFile(dbg).DateLastModified > fso.GetFile(rel).DateLastModified Then
    exe = dbg
  Else
    exe = rel
  End If
ElseIf fso.FileExists(rel) Then
  exe = rel
ElseIf fso.FileExists(dbg) Then
  exe = dbg
End If

If exe = "" Then
  MsgBox "No req-engine.exe found." & vbCrLf & _
    "Open a terminal:" & vbCrLf & _
    "  cd req-engine" & vbCrLf & _
    "  cargo build --release", 16, "Req-Engine"
  WScript.Quit 1
End If

sh.CurrentDirectory = engine
sh.Environment("Process")("REQ_ENGINE_SILENT") = "1"
' Same data home as the copy-pack MCP args: REQ_ENGINE_HOME or %USERPROFILE%\.req-engine
cmd = """" & exe & """ desktop --port 7420"
sh.Run cmd, 0, False
