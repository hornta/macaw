# Passive HID capability dump for Apple keyboards.
# Opens each HID top-level collection with zero access rights (works even for keyboard TLCs that
# Windows owns exclusively) and prints report IDs, usage pages and usages. Reads/writes no reports.
param([string]$Filter = 'vid_05ac')
$ErrorActionPreference = 'Stop'

$src = @'
using System;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;

public static class HidProbe
{
    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode)]
    static extern int CM_Get_Device_Interface_List_Size(out uint len, ref Guid g, string devId, uint flags);
    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode)]
    static extern int CM_Get_Device_Interface_List(ref Guid g, string devId, char[] buf, uint len, uint flags);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern SafeFileHandle CreateFile(string name, uint access, uint share, IntPtr sec, uint disp, uint flags, IntPtr tmpl);
    [DllImport("hid.dll")] [return: MarshalAs(UnmanagedType.U1)]
    static extern bool HidD_GetPreparsedData(SafeFileHandle h, out IntPtr pp);
    [DllImport("hid.dll")] [return: MarshalAs(UnmanagedType.U1)]
    static extern bool HidD_FreePreparsedData(IntPtr pp);
    [DllImport("hid.dll")] [return: MarshalAs(UnmanagedType.U1)]
    static extern bool HidD_GetAttributes(SafeFileHandle h, ref HIDD_ATTRIBUTES a);
    [DllImport("hid.dll")] [return: MarshalAs(UnmanagedType.U1)]
    static extern bool HidD_GetProductString(SafeFileHandle h, byte[] buf, int len);
    [DllImport("hid.dll")] [return: MarshalAs(UnmanagedType.U1)]
    static extern bool HidD_GetManufacturerString(SafeFileHandle h, byte[] buf, int len);
    [DllImport("hid.dll")] static extern int HidP_GetCaps(IntPtr pp, byte[] caps);
    [DllImport("hid.dll")] static extern int HidP_GetButtonCaps(int type, byte[] caps, ref ushort len, IntPtr pp);
    [DllImport("hid.dll")] static extern int HidP_GetValueCaps(int type, byte[] caps, ref ushort len, IntPtr pp);

    [StructLayout(LayoutKind.Sequential)]
    struct HIDD_ATTRIBUTES { public int Size; public ushort VendorID; public ushort ProductID; public ushort VersionNumber; }

    const int HIDP_STATUS_SUCCESS = 0x00110000;
    static ushort U16(byte[] b, int o) { return BitConverter.ToUInt16(b, o); }
    static int I32(byte[] b, int o) { return BitConverter.ToInt32(b, o); }
    static string Str(byte[] b) { return Encoding.Unicode.GetString(b).TrimEnd('\0'); }

    static string UsageText(byte[] c, int o)
    {
        bool isRange = c[o + 12] != 0;
        return isRange ? String.Format("0x{0:X4}-0x{1:X4}", U16(c, o + 56), U16(c, o + 58))
                       : String.Format("0x{0:X4}", U16(c, o + 56));
    }

    public static string Run(string filter)
    {
        var sb = new StringBuilder();
        Guid g = new Guid("4d1e55b2-f16f-11cf-88cb-001111000030"); // GUID_DEVINTERFACE_HID
        uint len;
        CM_Get_Device_Interface_List_Size(out len, ref g, null, 0);
        var buf = new char[len];
        CM_Get_Device_Interface_List(ref g, null, buf, len, 0);
        foreach (var p in new string(buf).Split(new[] { '\0' }, StringSplitOptions.RemoveEmptyEntries))
        {
            if (p.IndexOf(filter, StringComparison.OrdinalIgnoreCase) < 0) continue;
            sb.AppendLine("=== " + p);
            using (var h = CreateFile(p, 0, 3, IntPtr.Zero, 3, 0, IntPtr.Zero))
            {
                if (h.IsInvalid) { sb.AppendLine("  open failed, error " + Marshal.GetLastWin32Error()); continue; }
                var a = new HIDD_ATTRIBUTES(); a.Size = Marshal.SizeOf(a);
                if (HidD_GetAttributes(h, ref a))
                    sb.AppendLine(String.Format("  VID={0:X4} PID={1:X4} Ver={2:X4}", a.VendorID, a.ProductID, a.VersionNumber));
                var s = new byte[512];
                if (HidD_GetManufacturerString(h, s, s.Length)) sb.AppendLine("  Manufacturer: " + Str(s));
                s = new byte[512];
                if (HidD_GetProductString(h, s, s.Length)) sb.AppendLine("  Product: " + Str(s));
                IntPtr pp;
                if (!HidD_GetPreparsedData(h, out pp)) { sb.AppendLine("  no preparsed data"); continue; }
                try
                {
                    var caps = new byte[64];
                    HidP_GetCaps(pp, caps);
                    sb.AppendLine(String.Format("  TLC UsagePage=0x{0:X4} Usage=0x{1:X4}  InputLen={2} OutputLen={3} FeatureLen={4}",
                        U16(caps, 2), U16(caps, 0), U16(caps, 4), U16(caps, 6), U16(caps, 8)));
                    ushort[] nBtn = { U16(caps, 46), U16(caps, 52), U16(caps, 58) };
                    ushort[] nVal = { U16(caps, 48), U16(caps, 54), U16(caps, 60) };
                    string[] types = { "Input  ", "Output ", "Feature" };
                    for (int t = 0; t < 3; t++)
                    {
                        if (nBtn[t] > 0)
                        {
                            ushort n = nBtn[t]; var c = new byte[72 * n];
                            if (HidP_GetButtonCaps(t, c, ref n, pp) == HIDP_STATUS_SUCCESS)
                                for (int i = 0; i < n; i++)
                                {
                                    int o = 72 * i;
                                    sb.AppendLine(String.Format("    {0} BUTTON RID=0x{1:X2} Page=0x{2:X4} Usage={3}  [in collection page 0x{4:X4} usage 0x{5:X4}]",
                                        types[t], c[o + 2], U16(c, o), UsageText(c, o), U16(c, o + 10), U16(c, o + 8)));
                                }
                        }
                        if (nVal[t] > 0)
                        {
                            ushort n = nVal[t]; var c = new byte[72 * n];
                            if (HidP_GetValueCaps(t, c, ref n, pp) == HIDP_STATUS_SUCCESS)
                                for (int i = 0; i < n; i++)
                                {
                                    int o = 72 * i;
                                    sb.AppendLine(String.Format("    {0} VALUE  RID=0x{1:X2} Page=0x{2:X4} Usage={3} bits={4} count={5} logical=[{6},{7}]  [in collection page 0x{8:X4} usage 0x{9:X4}]",
                                        types[t], c[o + 2], U16(c, o), UsageText(c, o), U16(c, o + 18), U16(c, o + 20), I32(c, o + 40), I32(c, o + 44), U16(c, o + 10), U16(c, o + 8)));
                                }
                        }
                    }
                }
                finally { HidD_FreePreparsedData(pp); }
            }
        }
        return sb.ToString();
    }
}
'@

Add-Type -TypeDefinition $src -Language CSharp
[HidProbe]::Run($Filter)

Write-Output '=== Apple composite device (USB product string) ==='
Get-PnpDevice | Where-Object { $_.InstanceId -like 'USB\VID_05AC&PID_*' -and $_.InstanceId -notlike '*&MI_*' } | ForEach-Object {
  Write-Output ("  {0} -> {1}" -f $_.InstanceId, (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName 'DEVPKEY_Device_BusReportedDeviceDesc').Data)
}
Write-Output '=== Failed interface(s) ==='
Get-PnpDevice | Where-Object { $_.InstanceId -like 'USB\VID_05AC&PID_*&MI_*' -and $_.Status -ne 'OK' } | ForEach-Object {
  Write-Output ("  " + $_.InstanceId)
  foreach ($k in 'DEVPKEY_Device_ProblemCode', 'DEVPKEY_Device_ProblemStatus', 'DEVPKEY_Device_CompatibleIds', 'DEVPKEY_Device_Service', 'DEVPKEY_Device_BusReportedDeviceDesc') {
    $v = (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName $k -ErrorAction SilentlyContinue).Data
    Write-Output ("    {0} = {1}" -f $k, ($v -join ' | '))
  }
}
Write-Output '=== Bluetooth radio ==='
$bt = Get-PnpDevice -Class Bluetooth -PresentOnly -ErrorAction SilentlyContinue
if ($bt) { $bt | Format-Table Status, FriendlyName -AutoSize | Out-String -Width 200 } else { '  no Bluetooth devices/radio present' }
