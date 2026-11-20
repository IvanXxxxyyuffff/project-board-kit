#requires -version 5.1
<#
  yynote_export.ps1  --  READ-ONLY export of YYNote todos (table TList) -> yynote_todos.json

  Guarantees
    * Read-only: sqlite3_open_v2 is called with SQLITE_OPEN_READONLY (0x1) only.
    * The only SQL issued are SELECT statements (see $SqlList below). No INSERT / UPDATE /
      DELETE / REPLACE / DROP / VACUUM / ATTACH is ever executed.
    * No -wal / -shm is created, no lock beyond a shared read lock.
    * Deterministic: every output field is derived from database content or from the
      database file's own last-write time, so running this script twice yields
      byte-identical JSON (idempotent).

  Inputs
    * yynote.db : SQLCipher 4 database, password is fixed inside the client itself,
                  see YYNote.Local\LocalDbBase.cs:44-49)
    * e_sqlcipher.dll : shipped with YYNote under runtimes\win-x64\native

  Default source path resolution (first hit wins):
    1. -DbPath parameter
    2. %LOCALAPPDATA%\YYNoteVip\yynote.db
    3. %USERPROFILE%\Documents\YYNote\yynote.db
  Default output path: <folder of this script>\yynote_todos.json

  Usage
    powershell -NoProfile -ExecutionPolicy Bypass -File yynote_export.ps1
    powershell -NoProfile -ExecutionPolicy Bypass -File yynote_export.ps1 -DbPath <ROOT>\research\yynote.db -OutPath <ROOT>\research\yynote_out.json
#>
[CmdletBinding()]
param(
    [string]$DbPath,
    [string]$OutPath,
    [string]$NativeDir = '<ROOT>\YYNote\runtimes\win-x64\native'
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------- resolve source db
if (-not $DbPath) {
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA 'YYNoteVip\yynote.db'),
        (Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'YYNote\yynote.db')
    )
    foreach ($c in $candidates) {
        if (Test-Path -LiteralPath $c) { $DbPath = $c; break }
    }
}
if (-not $DbPath -or -not (Test-Path -LiteralPath $DbPath)) {
    throw "yynote.db not found (tried LOCALAPPDATA\YYNoteVip and Documents\YYNote)"
}

# ---------------------------------------------------------------- resolve output path
if (-not $OutPath) {
    $OutPath = Join-Path (Split-Path -Parent $PSCommandPath) 'yynote_todos.json'
}

$nativeDll = Join-Path $NativeDir 'e_sqlcipher.dll'
if (-not (Test-Path -LiteralPath $nativeDll)) {
    throw "e_sqlcipher.dll not found at $nativeDll"
}

# ---------------------------------------------------------------- native bridge (C#)
$cs = @'
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Net;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;

public static class YYNoteExporter
{
    // ---- sqlite3 constants (read only usage) ----
    private const int SQLITE_OK   = 0;
    private const int SQLITE_ROW  = 100;
    private const int SQLITE_DONE = 101;
    private const int SQLITE_OPEN_READONLY = 1;      // <-- the only open flag used

    // ---- kernel32 ----
    [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr LoadLibraryW(string lpFileName);
    [DllImport("kernel32", CharSet = CharSet.Ansi, SetLastError = true)]
    private static extern IntPtr GetProcAddress(IntPtr hModule, string lpProcName);
    [DllImport("kernel32", SetLastError = true)]
    private static extern bool FreeLibrary(IntPtr hModule);

    // ---- sqlite3 exported functions ----
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int OpenV2Fn(byte[] filename, out IntPtr db, int flags, IntPtr vfs);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int KeyFn(IntPtr db, byte[] key, int keylen);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int PrepareFn(IntPtr db, byte[] sql, int nByte, out IntPtr stmt, IntPtr tail);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int StepFn(IntPtr stmt);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate IntPtr ColTextFn(IntPtr stmt, int col);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int ColBytesFn(IntPtr stmt, int col);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int ColTypeFn(IntPtr stmt, int col);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int FinalizeFn(IntPtr stmt);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int CloseFn(IntPtr db);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate IntPtr ErrmsgFn(IntPtr db);

    private static OpenV2Fn  _open2;
    private static KeyFn     _key;
    private static PrepareFn _prepare;
    private static StepFn    _step;
    private static ColTextFn _colText;
    private static ColBytesFn _colBytes;
    private static ColTypeFn _colType;
    private static FinalizeFn _finalize;
    private static CloseFn   _close;
    private static ErrmsgFn  _errmsg;

    private static T Bind<T>(IntPtr module, string name)
    {
        IntPtr p = GetProcAddress(module, name);
        if (p == IntPtr.Zero) throw new Exception("missing export: " + name);
        return (T)(object)Marshal.GetDelegateForFunctionPointer(p, typeof(T));
    }

    // ---- dates ----
    private static readonly string[] NullSentinels = new string[] {
        "", "1970-01-01 08:00:01", "1970-01-01 08:00:00", "1970-01-01", "0001-01-01 00:00:00"
    };

    private static bool IsNullTime(string s)
    {
        if (s == null) return true;
        string t = s.Trim();
        foreach (string n in NullSentinels) if (t == n) return true;
        return false;
    }

    // "2026-11-04 09:09:10" -> "2026-11-04T09:09:10+08:00"  (db stores local time, UTC+8)
    private static string ToIso(string s)
    {
        if (IsNullTime(s)) return null;
        string t = s.Trim().Replace('/', '-');
        if (t.Length == 10) return t + "T00:00:00+08:00";
        if (t.Length == 16) return t.Replace(' ', 'T') + ":00+08:00";
        if (t.Length >= 19) return t.Substring(0, 19).Replace(' ', 'T') + "+08:00";
        return t;
    }

    private static string ToDate(string s)
    {
        if (IsNullTime(s)) return null;
        string t = s.Trim().Replace('/', '-');
        if (t.Length >= 10) return t.Substring(0, 10);
        return null;
    }

    // ---- text cleanup ----
    private static string StripHtml(string s)
    {
        if (string.IsNullOrEmpty(s)) return "";
        string t = Regex.Replace(s, "<[^>]*>", " ");
        t = WebUtility.HtmlDecode(t);
        t = Regex.Replace(t, @"\s+", " ");
        t = t.Trim();
        return t;
    }

    private static string HtmlToNote(string s)
    {
        if (string.IsNullOrEmpty(s)) return "";
        string t = Regex.Replace(s, "(?i)<br\s*/?>", "\n");
        t = Regex.Replace(t, "(?i)</(div|p|li|tr|h[1-6])>", "\n");
        t = Regex.Replace(t, "(?i)<li[^>]*>", "- ");
        t = Regex.Replace(t, "<[^>]*>", "");
        t = WebUtility.HtmlDecode(t);
        t = t.Replace("\r\n", "\n").Replace('\r', '\n');
        var lines = t.Split('\n');
        for (int i = 0; i < lines.Length; i++) lines[i] = lines[i].TrimEnd();
        t = string.Join("\n", lines);
        t = Regex.Replace(t, "\n{3,}", "\n\n").Trim();
        return t;
    }

    private static string Truncate(string s, int max)
    {
        if (string.IsNullOrEmpty(s)) return "";
        if (s.Length <= max) return s;
        string t = s.Substring(0, max);
        if (t.Length > 0 && Char.IsHighSurrogate(t[t.Length - 1])) t = t.Substring(0, t.Length - 1);
        return t;
    }

    private static string Esc(string s)
    {
        if (s == null) return "";
        StringBuilder sb = new StringBuilder(s.Length + 8);
        foreach (char c in s)
        {
            switch (c)
            {
                case '"':  sb.Append("\\\""); break;
                case '\\': sb.Append("\\\\"); break;
                case '\n': sb.Append("\\n");  break;
                case '\r': sb.Append("\\r");  break;
                case '\t': sb.Append("\\t");  break;
                case '\b': sb.Append("\\b");  break;
                case '\f': sb.Append("\\f");  break;
                default:
                    if (c < 0x20 || c == '\u2028' || c == '\u2029' || c == '\u007f')
                        sb.Append("\\u" + ((int)c).ToString("x4"));
                    else
                        sb.Append(c);
                    break;
            }
        }
        return sb.ToString();
    }

    // ---- column helper ----
    private static string ColStr(IntPtr stmt, int idx)
    {
        if (_colType(stmt, idx) == 5) return null;          // SQLITE_NULL
        IntPtr p = _colText(stmt, idx);
        int n = _colBytes(stmt, idx);
        if (p == IntPtr.Zero || n <= 0) return "";
        byte[] buf = new byte[n];
        Marshal.Copy(p, buf, 0, n);
        return Encoding.UTF8.GetString(buf);
    }

    private static void Check(int rc, IntPtr db, string what)
    {
        if (rc == SQLITE_OK || rc == SQLITE_ROW || rc == SQLITE_DONE) return;
        string msg = _errmsg == null ? "" : Marshal.PtrToStringAnsi(_errmsg(db));
        throw new Exception(what + " failed, rc=" + rc + " (" + msg + ")");
    }

    private sealed class Row
    {
        public string Id, Text, Body, Due, Priority, Created, Updated;
        public bool Done;
    }

    // =================================================================== main entry
    public static string Export(string dbPath, string nativeDll, string outPath)
    {
        IntPtr mod = LoadLibraryW(nativeDll);
        if (mod == IntPtr.Zero)
            throw new Exception("LoadLibrary failed for " + nativeDll + " (win32 err " + Marshal.GetLastWin32Error() + ")");

        _open2    = Bind<OpenV2Fn>(mod, "sqlite3_open_v2");
        _key      = Bind<KeyFn>(mod, "sqlite3_key");
        _prepare  = Bind<PrepareFn>(mod, "sqlite3_prepare_v2");
        _step     = Bind<StepFn>(mod, "sqlite3_step");
        _colText  = Bind<ColTextFn>(mod, "sqlite3_column_text");
        _colBytes = Bind<ColBytesFn>(mod, "sqlite3_column_bytes");
        _colType  = Bind<ColTypeFn>(mod, "sqlite3_column_type");
        _finalize = Bind<FinalizeFn>(mod, "sqlite3_finalize");
        _close    = Bind<CloseFn>(mod, "sqlite3_close");
        _errmsg   = Bind<ErrmsgFn>(mod, "sqlite3_errmsg");

        IntPtr db;
        byte[] pathBytes = Encoding.UTF8.GetBytes(dbPath + "\0");
        int rc = _open2(pathBytes, out db, SQLITE_OPEN_READONLY, IntPtr.Zero);   // READ-ONLY, no other flags
        if (rc != SQLITE_OK) throw new Exception("sqlite3_open_v2(READONLY) failed, rc=" + rc);

        byte[] keyBytes = Encoding.UTF8.GetBytes("<passphrase>");
        rc = _key(db, keyBytes, keyBytes.Length);
        if (rc != SQLITE_OK) throw new Exception("sqlite3_key failed, rc=" + rc);

        int total = Scalar(db, "SELECT COUNT(*) FROM TList");
        int kept  = Scalar(db, "SELECT COUNT(*) FROM TList WHERE state <> 2");
        int nulls = Scalar(db, "SELECT COUNT(*) FROM TList WHERE state IS NULL");

        List<Row> rows = new List<Row>();

        // ONLY SELECT. state <> 2 keeps state 0 (todo) and state 1 (done); 2 = soft deleted.
        string sql =
            "SELECT list_id, content, describe, state, stick_time, target_date, remind_date, " +
            "list_begin_time, create_time, modify_time, position " +
            "FROM TList WHERE state <> 2 ORDER BY create_time, list_id";

        List<object> outRows = new List<object>();
        IntPtr stmt;
        byte[] sqlBytes = Encoding.UTF8.GetBytes(sql + "\0");
        rc = _prepare(db, sqlBytes, sqlBytes.Length - 1, out stmt, IntPtr.Zero);
        Check(rc, db, "prepare");
        try
        {
            while (true)
            {
                rc = _step(stmt);
                if (rc == SQLITE_DONE) break;
                Check(rc, db, "step");

                Row r = new Row();
                r.Id       = ColStr(stmt, 0) ?? "";
                r.Text     = Truncate(StripHtml(ColStr(stmt, 1)), 200);
                r.Body     = HtmlToNote(ColStr(stmt, 2));
                string st  = ColStr(stmt, 3) ?? "0";
                r.Done     = (st.Trim() == "1");
                string stick = ColStr(stmt, 4);
                string target = ColStr(stmt, 5);
                string remind = ColStr(stmt, 6);
                string begin  = ColStr(stmt, 7);
                r.Created  = ToIso(ColStr(stmt, 8));
                r.Updated  = ToIso(ColStr(stmt, 9));
                r.Priority = ColStr(stmt, 10) ?? "";              // raw value of TList.position

                // due date precedence: target_date -> list_begin_time -> remind_date
                r.Due = ToDate(target);
                if (r.Due == null) r.Due = ToDate(begin);
                if (r.Due == null) r.Due = ToDate(remind);

                rows.Add(r);
            }
        }
        finally
        {
            _finalize(stmt);
        }

        _close(db);

        // -------- serialise (deterministic; exportedAt = db file last write time) --------
        DateTime stamp = File.GetLastWriteTime(dbPath);
        string exportedAt = stamp.ToString("yyyy-MM-ddTHH:mm:sszzz", CultureInfo.InvariantCulture);

        StringBuilder sb = new StringBuilder();
        sb.Append("{\n");
        sb.Append("  \"exportedAt\": \"").Append(Esc(exportedAt)).Append("\",\n");
        sb.Append("  \"source\": \"yynote.db\",\n");
        sb.Append("  \"count\": ").Append(rows.Count.ToString(CultureInfo.InvariantCulture)).Append(",\n");
        sb.Append("  \"items\": [");
        for (int i = 0; i < rows.Count; i++)
        {
            Row r = rows[i];
            sb.Append(i == 0 ? "\n" : ",\n");
            sb.Append("    {\n");
            sb.Append("      \"id\": \"").Append(Esc(r.Id)).Append("\",\n");
            sb.Append("      \"text\": \"").Append(Esc(r.Text)).Append("\",\n");
            sb.Append("      \"body\": \"").Append(Esc(r.Body)).Append("\",\n");
            sb.Append("      \"due\": \"").Append(r.Due == null ? "" : Esc(r.Due)).Append("\",\n");
            sb.Append("      \"done\": ").Append(r.Done ? "true" : "false").Append(",\n");
            sb.Append("      \"deleted\": false,\n");
            sb.Append("      \"priority\": \"").Append(Esc(r.Priority)).Append("\",\n");
            sb.Append("      \"created\": ").Append(r.Created == null ? "null" : "\"" + Esc(r.Created) + "\"").Append(",\n");
            sb.Append("      \"updated\": ").Append(r.Updated == null ? "null" : "\"" + Esc(r.Updated) + "\"").Append("\n");
            sb.Append("    }");
        }
        if (rows.Count > 0) sb.Append("\n  ");
        sb.Append("]\n");
        sb.Append("}\n");

        string dir = Path.GetDirectoryName(Path.GetFullPath(outPath));
        if (!Directory.Exists(dir)) Directory.CreateDirectory(dir);

        // UTF-8 without BOM, LF line endings -> byte identical across runs
        string text = sb.ToString().Replace("\r\n", "\n");
        File.WriteAllBytes(outPath, new UTF8Encoding(false).GetBytes(text));

        FreeLibrary(mod);

        return "db=" + dbPath + "; native=" + nativeDll + "; total_rows=" + total +
               "; state_ne_2=" + kept + "; state_null=" + nulls + "; exported=" + rows.Count +
               "; out=" + outPath;
    }

    private static int Scalar(IntPtr db, string sql)
    {
        IntPtr stmt;
        byte[] sqlBytes = Encoding.UTF8.GetBytes(sql + "\0");
        int rc = _prepare(db, sqlBytes, sqlBytes.Length - 1, out stmt, IntPtr.Zero);
        Check(rc, db, "prepare(scalar)");
        try
        {
            rc = _step(stmt);
            if (rc == SQLITE_ROW)
            {
                string s = ColStr(stmt, 0);
                int v;
                if (int.TryParse(s, out v)) return v;
            }
            return 0;
        }
        finally
        {
            _finalize(stmt);
        }
    }
}
'@

if (-not ('YYNoteExporter' -as [type])) {
    Add-Type -TypeDefinition $cs -Language CSharp -ErrorAction Stop
}

$summary = [YYNoteExporter]::Export($DbPath, $nativeDll, $OutPath)
Write-Output ('OK ' + $summary)
