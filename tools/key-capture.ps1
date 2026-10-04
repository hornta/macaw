# mkaw key capture: logs what Windows actually receives for each key press
#  - LL   = WH_KEYBOARD_LL hook event (what remappers like PowerToys/AutoHotkey see)
#  - RAW  = Raw Input event (carries the source device); RAW* = drained from the queue *inside* the LL hook
#  - HID  = raw report from Apple's consumer / vendor-defined collections
# Letters and digits (except Q and 2) are redacted. Nothing leaves this machine.
param(
  [string]$OutFile = (Join-Path $PSScriptRoot 'key-capture.log'),
  [int]$Seconds = 180,
  [switch]$FlipMode,     # optional: temporarily flip the keyboard's feature-report 0x09 flag; restored on exit
  [switch]$SwallowSection, # optional: the test hook blocks the key left of 1, to see whether Raw Input still reports it
  [switch]$CompileOnly
)
$ErrorActionPreference = 'Stop'

$src = @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;

public static class KeyCapture
{
    delegate IntPtr WndProcFn(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);
    delegate IntPtr HookFn(int code, IntPtr w, IntPtr l);
    delegate bool CtrlFn(uint type);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct WNDCLASSEX
    {
        public uint cbSize; public uint style; public WndProcFn lpfnWndProc; public int cbClsExtra; public int cbWndExtra;
        public IntPtr hInstance; public IntPtr hIcon; public IntPtr hCursor; public IntPtr hbrBackground;
        public string lpszMenuName; public string lpszClassName; public IntPtr hIconSm;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam; public IntPtr lParam; public uint time; public int x; public int y; }
    [StructLayout(LayoutKind.Sequential)]
    struct RAWINPUTDEVICE { public ushort UsagePage; public ushort Usage; public uint Flags; public IntPtr Target; }
    [StructLayout(LayoutKind.Sequential)]
    struct KBDLLHOOKSTRUCT { public uint vkCode; public uint scanCode; public uint flags; public uint time; public UIntPtr extra; }

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern ushort RegisterClassEx(ref WNDCLASSEX wc);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr CreateWindowEx(uint exStyle, string cls, string title, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr param);
    [DllImport("user32.dll")] static extern IntPtr DefWindowProc(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern int GetMessage(out MSG m, IntPtr hwnd, uint min, uint max);
    [DllImport("user32.dll")] static extern bool PeekMessage(out MSG m, IntPtr hwnd, uint min, uint max, uint remove);
    [DllImport("user32.dll")] static extern bool TranslateMessage(ref MSG m);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessage(ref MSG m);
    [DllImport("user32.dll")] static extern void PostQuitMessage(int code);
    [DllImport("user32.dll")] static extern IntPtr SetTimer(IntPtr hwnd, IntPtr id, uint ms, IntPtr fn);
    [DllImport("user32.dll", SetLastError = true)] static extern bool RegisterRawInputDevices(RAWINPUTDEVICE[] d, uint n, uint cb);
    [DllImport("user32.dll")] static extern uint GetRawInputData(IntPtr h, uint cmd, byte[] data, ref uint size, uint cbHeader);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern uint GetRawInputDeviceInfo(IntPtr dev, uint cmd, StringBuilder data, ref uint size);
    [DllImport("user32.dll", SetLastError = true)] static extern IntPtr SetWindowsHookEx(int id, HookFn fn, IntPtr mod, uint tid);
    [DllImport("user32.dll")] static extern bool UnhookWindowsHookEx(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr CallNextHookEx(IntPtr h, int n, IntPtr w, IntPtr l);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern IntPtr GetModuleHandle(string name);
    [DllImport("kernel32.dll")] static extern bool SetConsoleCtrlHandler(CtrlFn fn, bool add);
    [DllImport("user32.dll")] static extern short GetAsyncKeyState(int vk);

    // keys whose logical state we poll, to catch keys Windows believes are still held
    static readonly uint[] watch = { 0x08, 0x0D, 0x25, 0x26, 0x27, 0x28, 0x2E, 0x24, 0x23, 0x21, 0x22, 0x2D,
                                     0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x7B, 0xDC, 0xC0, 0xE2 };
    static readonly bool[] watchDown = new bool[watch.Length];
    static bool swallowSection;

    const uint WM_INPUT = 0x00FF, WM_TIMER = 0x0113;
    const uint RID_INPUT = 0x10000003, RIDI_DEVICENAME = 0x20000007;
    const uint RIDEV_INPUTSINK = 0x100, RIDEV_PAGEONLY = 0x20, PM_REMOVE = 1;

    static WndProcFn wndProc; static HookFn hookFn; static CtrlFn ctrlFn;   // keep delegates alive
    static IntPtr hwnd, hook;
    static readonly Stopwatch clock = new Stopwatch();
    static readonly Dictionary<IntPtr, string> names = new Dictionary<IntPtr, string>();
    static readonly Dictionary<string, int> perDevice = new Dictionary<string, int>();
    static readonly object gate = new object();
    static StreamWriter file;
    static string lastRawDev; static int lastRawMake; static bool lastRawUp, haveRaw;
    static int llTotal, llQueued, llMatched, llMismatched, llInjected, qCount;
    static bool finished;

    static void Log(string s)
    {
        lock (gate)
        {
            string line = String.Format("{0,10:F2} ms  {1}", clock.Elapsed.TotalMilliseconds, s);
            Console.WriteLine(line);
            if (file != null) { file.WriteLine(line); file.Flush(); }
        }
    }

    public static void Run(string outFile, bool flip, int seconds, bool swallow)
    {
        swallowSection = swallow;
        file = new StreamWriter(outFile, false, new UTF8Encoding(false));
        clock.Start();
        wndProc = WndProc; hookFn = Hook; ctrlFn = OnCtrl;
        SetConsoleCtrlHandler(ctrlFn, true);

        var wc = new WNDCLASSEX();
        wc.cbSize = (uint)Marshal.SizeOf(typeof(WNDCLASSEX));
        wc.lpfnWndProc = wndProc;
        wc.hInstance = GetModuleHandle(null);
        wc.lpszClassName = "mkawKeyCapture";
        RegisterClassEx(ref wc);
        hwnd = CreateWindowEx(0, "mkawKeyCapture", "mkaw key capture", 0, 0, 0, 0, 0, IntPtr.Zero, IntPtr.Zero, wc.hInstance, IntPtr.Zero);
        Log("hidden window: " + (hwnd == IntPtr.Zero ? "FAILED " + Marshal.GetLastWin32Error() : "ok"));

        Register(0x0001, 0x0006, RIDEV_INPUTSINK, "keyboards");
        Register(0x000C, 0x0001, RIDEV_INPUTSINK, "consumer control");
        Register(0xFF00, 0x0000, RIDEV_INPUTSINK | RIDEV_PAGEONLY, "vendor page 0xFF00");
        Register(0xFF01, 0x0000, RIDEV_INPUTSINK | RIDEV_PAGEONLY, "vendor page 0xFF01");
        Register(0x00FF, 0x0000, RIDEV_INPUTSINK | RIDEV_PAGEONLY, "vendor page 0x00FF");

        hook = SetWindowsHookEx(13, hookFn, GetModuleHandle(null), 0);
        Log("low-level keyboard hook: " + (hook == IntPtr.Zero ? "FAILED " + Marshal.GetLastWin32Error() : "ok"));
        Log(AppleFeature.Probe(flip));
        SetTimer(hwnd, new IntPtr(1), (uint)seconds * 1000u, IntPtr.Zero);
        SetTimer(hwnd, new IntPtr(2), 200u, IntPtr.Zero);
        if (swallowSection) Log("test hook will SWALLOW the key left of 1 (scan code 0x29)");
        Log(String.Format("capturing for up to {0} s; press Q three times to finish", seconds));

        MSG m;
        while (GetMessage(out m, IntPtr.Zero, 0, 0) > 0) { TranslateMessage(ref m); DispatchMessage(ref m); }
        Finish();
    }

    static void Register(ushort page, ushort usage, uint flags, string label)
    {
        var d = new[] { new RAWINPUTDEVICE { UsagePage = page, Usage = usage, Flags = flags, Target = hwnd } };
        bool ok = RegisterRawInputDevices(d, 1, (uint)Marshal.SizeOf(typeof(RAWINPUTDEVICE)));
        Log(String.Format("raw input {0,-20}: {1}", label, ok ? "ok" : "FAILED " + Marshal.GetLastWin32Error()));
    }

    static IntPtr WndProc(IntPtr h, uint msg, IntPtr w, IntPtr l)
    {
        if (msg == WM_INPUT) Raw(l, "RAW ", false);
        else if (msg == WM_TIMER)
        {
            if (w == new IntPtr(2)) PollKeys(); else PostQuitMessage(0);
            return IntPtr.Zero;
        }
        return DefWindowProc(h, msg, w, l);
    }

    static void PollKeys()
    {
        for (int i = 0; i < watch.Length; i++)
        {
            bool down = (GetAsyncKeyState((int)watch[i]) & 0x8000) != 0;
            if (down == watchDown[i]) continue;
            watchDown[i] = down;
            Log(String.Format("STATE Windows now considers {0} {1}", VkName(watch[i]), down ? "HELD DOWN" : "released"));
        }
    }

    static IntPtr Hook(int code, IntPtr w, IntPtr l)
    {
        if (code >= 0)
        {
            var k = (KBDLLHOOKSTRUCT)Marshal.PtrToStructure(l, typeof(KBDLLHOOKSTRUCT));
            // Is the WM_INPUT for this very keystroke already queued? Drain and inspect it.
            haveRaw = false;
            MSG m;
            while (PeekMessage(out m, hwnd, WM_INPUT, WM_INPUT, PM_REMOVE))
            {
                Raw(m.lParam, "RAW*", true);
                DefWindowProc(m.hwnd, m.message, m.wParam, m.lParam);
            }
            bool up = (k.flags & 0x80) != 0;
            llTotal++;
            if ((k.flags & 0x10) != 0) llInjected++;
            string attrib;
            if (!haveRaw) attrib = "no WM_INPUT queued yet";
            else
            {
                llQueued++;
                bool match = lastRawMake == (int)(k.scanCode & 0xFF) && lastRawUp == up;
                if (match) { llMatched++; attrib = "from " + lastRawDev; }
                else { llMismatched++; attrib = "MISMATCH (last queued raw: " + lastRawDev + ")"; }
            }
            string key = Redacted(k.vkCode) ? "(letter/digit redacted)" : String.Format("vk=0x{0:X2} {1} sc=0x{2:X2}", k.vkCode, VkName(k.vkCode), k.scanCode);
            Log(String.Format("LL   {0,-4} {1} flags=[{2}] extra=0x{3:X} -> {4}", up ? "up" : "down", key, LlFlags(k.flags), k.extra.ToUInt64(), attrib));
            if (!up) { if (k.vkCode == 0x51) { if (++qCount >= 3) PostQuitMessage(0); } else qCount = 0; }
            if (swallowSection && (k.scanCode & 0xFF) == 0x29 && (k.flags & 0x10) == 0)
            {
                Log("LL   ^ SWALLOWED by the test hook; does a RAW line for it still arrive?");
                return new IntPtr(1);
            }
        }
        return CallNextHookEx(hook, code, w, l);
    }

    static void Raw(IntPtr hRaw, string tag, bool inHook)
    {
        uint hdr = (uint)(8 + 2 * IntPtr.Size), size = 0;
        GetRawInputData(hRaw, RID_INPUT, null, ref size, hdr);
        if (size == 0) return;
        var b = new byte[size];
        if (GetRawInputData(hRaw, RID_INPUT, b, ref size, hdr) == 0xFFFFFFFF) return;
        uint type = BitConverter.ToUInt32(b, 0);
        IntPtr dev = IntPtr.Size == 8 ? new IntPtr(BitConverter.ToInt64(b, 8)) : new IntPtr(BitConverter.ToInt32(b, 8));
        int o = (int)hdr;
        string dn = DevName(dev);
        int c; perDevice.TryGetValue(dn, out c); perDevice[dn] = c + 1;
        if (type == 1)
        {
            ushort make = BitConverter.ToUInt16(b, o), fl = BitConverter.ToUInt16(b, o + 2), vk = BitConverter.ToUInt16(b, o + 6);
            uint extra = BitConverter.ToUInt32(b, o + 12);
            bool up = (fl & 1) != 0;
            string prefix = (fl & 2) != 0 ? "E0 " : ((fl & 4) != 0 ? "E1 " : "");
            string key = Redacted(vk) ? "(letter/digit redacted)" : String.Format("make={0}0x{1:X2} vk=0x{2:X2} {3}", prefix, make, vk, VkName(vk));
            Log(String.Format("{0} KBD {1,-4} dev={2,-22} {3} extra=0x{4:X}", tag, up ? "up" : "down", dn, key, extra));
            if (inHook) { haveRaw = true; lastRawDev = dn; lastRawMake = make; lastRawUp = up; }
        }
        else if (type == 2)
        {
            uint sz = BitConverter.ToUInt32(b, o), cnt = BitConverter.ToUInt32(b, o + 4);
            for (uint i = 0; i < cnt; i++)
            {
                int start = o + 8 + (int)(i * sz);
                int n = (int)Math.Min(sz, 32u);
                if (start + n > b.Length) break;
                Log(String.Format("{0} HID dev={1,-22} len={2,-3} {3}{4}", tag, dn, sz, BitConverter.ToString(b, start, n).Replace('-', ' '), sz > 32 ? " ..." : ""));
            }
        }
    }

    static string DevName(IntPtr dev)
    {
        if (dev == IntPtr.Zero) return "(no device: injected)";
        string n;
        if (names.TryGetValue(dev, out n)) return n;
        uint size = 0;
        GetRawInputDeviceInfo(dev, RIDI_DEVICENAME, null, ref size);
        var sb = new StringBuilder((int)size + 2);
        GetRawInputDeviceInfo(dev, RIDI_DEVICENAME, sb, ref size);
        string path = sb.ToString();
        n = Short(path);
        names[dev] = n;
        Log("new device " + n + "  =  " + path);
        return n;
    }

    static string Short(string p)
    {
        string u = p.ToUpperInvariant();
        string vid = Grab(u, "VID_", 4), pid = Grab(u, "PID_", 4);
        if (vid == null) { vid = Grab(u, "VID&0001", 4) ?? Grab(u, "VID&02", 4); pid = Grab(u, "PID&", 4); }
        string s = (vid ?? "????") + ":" + (pid ?? "????");
        string mi = Grab(u, "&MI_", 2), col = Grab(u, "&COL", 2);
        if (mi != null) s += " MI" + mi;
        if (col != null) s += " C" + col;
        if (vid == "05AC" || vid == "004C") s = "APPLE " + s;
        if (u.Contains("{00001124-")) s += " (BT)";
        if (u.Contains("{00001812-")) s += " (BLE)";
        return s;
    }

    static string Grab(string s, string key, int len)
    {
        int i = s.IndexOf(key, StringComparison.Ordinal);
        if (i < 0 || i + key.Length + len > s.Length) return null;
        return s.Substring(i + key.Length, len);
    }

    static bool Redacted(uint vk)
    {
        return (vk >= 0x41 && vk <= 0x5A && vk != 0x51) || (vk >= 0x30 && vk <= 0x39 && vk != 0x32) || (vk >= 0x60 && vk <= 0x69);
    }

    static string LlFlags(uint f)
    {
        var s = new List<string>();
        if ((f & 0x01) != 0) s.Add("ext");
        if ((f & 0x02) != 0) s.Add("lowerIL-injected");
        if ((f & 0x10) != 0) s.Add("injected");
        if ((f & 0x20) != 0) s.Add("alt");
        return String.Join(",", s);
    }

    static string VkName(uint vk)
    {
        if (vk >= 0x41 && vk <= 0x5A) return ((char)vk).ToString();
        if (vk >= 0x30 && vk <= 0x39) return ((char)vk).ToString();
        if (vk >= 0x70 && vk <= 0x87) return "F" + (vk - 0x6F);
        if (vk >= 0x60 && vk <= 0x69) return "Num" + (vk - 0x60);
        switch (vk)
        {
            case 0x08: return "Backspace"; case 0x09: return "Tab"; case 0x0C: return "Clear"; case 0x0D: return "Enter";
            case 0x10: return "Shift"; case 0x11: return "Ctrl"; case 0x12: return "Alt"; case 0x13: return "Pause"; case 0x14: return "CapsLock";
            case 0x1B: return "Esc"; case 0x20: return "Space"; case 0x21: return "PgUp"; case 0x22: return "PgDn"; case 0x23: return "End"; case 0x24: return "Home";
            case 0x25: return "Left"; case 0x26: return "Up"; case 0x27: return "Right"; case 0x28: return "Down";
            case 0x2C: return "PrtSc"; case 0x2D: return "Insert"; case 0x2E: return "Delete";
            case 0x5B: return "LWin"; case 0x5C: return "RWin"; case 0x5D: return "Apps"; case 0x5F: return "Sleep";
            case 0x90: return "NumLock"; case 0x91: return "ScrollLock";
            case 0xA0: return "LShift"; case 0xA1: return "RShift"; case 0xA2: return "LCtrl"; case 0xA3: return "RCtrl"; case 0xA4: return "LAlt"; case 0xA5: return "RAlt";
            case 0xA6: return "BrowserBack"; case 0xA7: return "BrowserFwd"; case 0xAA: return "BrowserSearch"; case 0xAC: return "BrowserHome";
            case 0xAD: return "VolMute"; case 0xAE: return "VolDown"; case 0xAF: return "VolUp";
            case 0xB0: return "MediaNext"; case 0xB1: return "MediaPrev"; case 0xB2: return "MediaStop"; case 0xB3: return "MediaPlayPause";
            case 0xB4: return "LaunchMail"; case 0xB5: return "LaunchMedia"; case 0xB6: return "LaunchApp1"; case 0xB7: return "LaunchApp2";
            case 0xBA: return "OEM_1"; case 0xBB: return "OEM_PLUS"; case 0xBC: return "OEM_COMMA"; case 0xBD: return "OEM_MINUS"; case 0xBE: return "OEM_PERIOD";
            case 0xBF: return "OEM_2"; case 0xC0: return "OEM_3"; case 0xDB: return "OEM_4"; case 0xDC: return "OEM_5"; case 0xDD: return "OEM_6";
            case 0xDE: return "OEM_7"; case 0xDF: return "OEM_8"; case 0xE2: return "OEM_102"; case 0xE7: return "Packet"; case 0xFF: return "(none)";
        }
        return "?";
    }

    static void Finish()
    {
        lock (gate) { if (finished) return; finished = true; }
        if (hook != IntPtr.Zero) { UnhookWindowsHookEx(hook); hook = IntPtr.Zero; }
        string r = AppleFeature.Restore();
        if (r != null) Log(r);
        Log(String.Format("SUMMARY  LL events: {0} (injected: {1}); WM_INPUT already queued when the LL hook ran: {2}; attributed to a device: {3}; mismatched: {4}",
            llTotal, llInjected, llQueued, llMatched, llMismatched));
        foreach (var kv in perDevice) Log(String.Format("SUMMARY  raw events from {0}: {1}", kv.Key, kv.Value));
        lock (gate) { if (file != null) { file.Flush(); file.Dispose(); file = null; } }
    }

    static bool OnCtrl(uint type)
    {
        if (type == 0) return true;   // Ctrl+C: ignore, keep capturing
        Finish();
        return false;
    }
}

public static class AppleFeature
{
    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode)] static extern int CM_Get_Device_Interface_List_Size(out uint len, ref Guid g, string devId, uint flags);
    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode)] static extern int CM_Get_Device_Interface_List(ref Guid g, string devId, char[] buf, uint len, uint flags);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern SafeFileHandle CreateFile(string name, uint access, uint share, IntPtr sec, uint disp, uint flags, IntPtr tmpl);
    [DllImport("hid.dll", SetLastError = true)] [return: MarshalAs(UnmanagedType.U1)] static extern bool HidD_GetFeature(SafeFileHandle h, byte[] buf, int len);
    [DllImport("hid.dll", SetLastError = true)] [return: MarshalAs(UnmanagedType.U1)] static extern bool HidD_SetFeature(SafeFileHandle h, byte[] buf, int len);
    [DllImport("hid.dll")] [return: MarshalAs(UnmanagedType.U1)] static extern bool HidD_GetPreparsedData(SafeFileHandle h, out IntPtr pp);
    [DllImport("hid.dll")] [return: MarshalAs(UnmanagedType.U1)] static extern bool HidD_FreePreparsedData(IntPtr pp);
    [DllImport("hid.dll")] static extern int HidP_GetCaps(IntPtr pp, byte[] caps);
    [DllImport("hid.dll")] static extern int HidP_GetUsageValue(int type, ushort page, ushort link, ushort usage, out uint value, IntPtr pp, byte[] report, uint len);
    [DllImport("hid.dll")] static extern int HidP_SetUsageValue(int type, ushort page, ushort link, ushort usage, uint value, IntPtr pp, byte[] report, uint len);
    const int OK = 0x00110000;

    static SafeFileHandle held; static IntPtr heldPp; static byte[] originalReport; static uint originalValue; static bool flipped;

    static string FindPath()
    {
        Guid g = new Guid("4d1e55b2-f16f-11cf-88cb-001111000030");
        uint len;
        if (CM_Get_Device_Interface_List_Size(out len, ref g, null, 0) != 0 || len == 0) return null;
        var buf = new char[len];
        if (CM_Get_Device_Interface_List(ref g, null, buf, len, 0) != 0) return null;
        foreach (var p in new string(buf).Split('\0'))
        {
            string u = p.ToUpperInvariant();
            if (u.Contains("VID_05AC") && u.Contains("&COL02")) return p;
        }
        return null;
    }

    public static string Probe(bool flip)
    {
        string path = FindPath();
        if (path == null) return "feature 0x09: Apple consumer collection (Col02) not found";
        string result = "feature 0x09: could not be read";
        uint[] order = flip ? new uint[] { 0xC0000000u, 0u } : new uint[] { 0u, 0xC0000000u };
        foreach (uint access in order)
        {
            var h = CreateFile(path, access, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
            if (h.IsInvalid) { result = String.Format("feature 0x09: open (access 0x{0:X}) failed, error {1}", access, Marshal.GetLastWin32Error()); continue; }
            IntPtr pp;
            if (!HidD_GetPreparsedData(h, out pp)) { h.Dispose(); continue; }
            var caps = new byte[64];
            HidP_GetCaps(pp, caps);
            int len = BitConverter.ToUInt16(caps, 8);
            var buf = new byte[len];
            buf[0] = 0x09;
            if (!HidD_GetFeature(h, buf, len))
            {
                result = String.Format("feature 0x09: HidD_GetFeature (access 0x{0:X}) failed, error {1}", access, Marshal.GetLastWin32Error());
                HidD_FreePreparsedData(pp); h.Dispose();
                continue;
            }
            uint v;
            int st = HidP_GetUsageValue(2, 0xFF01, 0, 0x000B, out v, pp, buf, (uint)len);
            result = String.Format("feature 0x09 (page 0xFF01 usage 0x0B) read via access 0x{0:X}: bytes [{1}] -> value {2}",
                access, BitConverter.ToString(buf), st == OK ? v.ToString() : "? (decode status 0x" + st.ToString("X") + ")");
            if (flip && st == OK)
            {
                uint nv = v == 0 ? 1u : 0u;
                var nb = (byte[])buf.Clone();
                bool ok = HidP_SetUsageValue(2, 0xFF01, 0, 0x000B, nv, pp, nb, (uint)len) == OK && HidD_SetFeature(h, nb, len);
                result += String.Format("\n             FLIP value {0} -> {1}: {2}", v, nv, ok ? "ok (restored on exit)" : "FAILED, error " + Marshal.GetLastWin32Error());
                if (ok) { held = h; heldPp = pp; originalReport = buf; originalValue = v; flipped = true; return result; }
            }
            HidD_FreePreparsedData(pp); h.Dispose();
            return result;
        }
        return result;
    }

    public static string Restore()
    {
        if (!flipped) return null;
        flipped = false;
        bool ok = HidD_SetFeature(held, originalReport, originalReport.Length);
        var check = new byte[originalReport.Length];
        check[0] = 0x09;
        uint v = 999;
        if (HidD_GetFeature(held, check, check.Length)) HidP_GetUsageValue(2, 0xFF01, 0, 0x000B, out v, heldPp, check, (uint)check.Length);
        HidD_FreePreparsedData(heldPp); held.Dispose();
        return String.Format("restored feature 0x09 to value {0}: {1}; read back: {2}", originalValue, ok ? "ok" : "FAILED", v == 999 ? "unreadable" : v.ToString());
    }
}
'@

Add-Type -TypeDefinition $src -Language CSharp
if ($CompileOnly) { 'compiled ok'; return }

$Host.UI.RawUI.WindowTitle = 'mkaw key capture'
Write-Host ''
Write-Host ' mkaw key capture - records key CODES only (letters/digits redacted) to:' -ForegroundColor Cyan
Write-Host "   $OutFile"
if ($FlipMode) {
  Write-Host ' FOLLOW-UP TEST - the keyboard flag (feature report 0x09) is inverted now and restored at the end.' -ForegroundColor Magenta
  Write-Host ' Focus does not matter. Press slowly:' -ForegroundColor Cyan
  Write-Host '  1  F1 ... F12 WITHOUT fn   (notice whether volume/media react this time)'
  Write-Host '  2  hold fn, tap Backspace, wait a second, then release fn'
  Write-Host '  3  press fn+Backspace TOGETHER and release them TOGETHER'
  Write-Host '  4  same as 3 with fn+Left arrow, then fn+Up arrow'
  Write-Host '  5  the key left of 1, three times  (this run blocks it on purpose, it will type nothing)'
  Write-Host '  6  Q three times to finish' -ForegroundColor Yellow
} else {
  Write-Host ' Keep this window focused. Press slowly, one key at a time:' -ForegroundColor Cyan
  Write-Host '  1  fn (globe) alone, twice'
  Write-Host '  2  F1 ... F12 WITHOUT fn   (volume/media may really change - expected)'
  Write-Host '  3  fn+F1, fn+F2, fn+F7, fn+F8, fn+F10, fn+F12'
  Write-Host '  4  fn+Backspace, fn+Return, fn+Left arrow, fn+Up arrow'
  Write-Host '  5  the Touch ID key (top right), once'
  Write-Host '  6  left control, left option, left command, right command, right option  (Start may open: press command again)'
  Write-Host '  7  Caps Lock twice'
  Write-Host '  8  the key left of 1 (section sign), then the key left of Z (less-than)'
  Write-Host '  9  right option + 2'
  Write-Host ' 10  Q three times to finish (stops by itself after the time limit)' -ForegroundColor Yellow
}
Write-Host ''
[KeyCapture]::Run($OutFile, $FlipMode.IsPresent, $Seconds, $SwallowSection.IsPresent)
Write-Host ''
Write-Host ' Done. You can close this window.' -ForegroundColor Green
