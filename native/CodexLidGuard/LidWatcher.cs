using System.ComponentModel;
using System.Runtime.InteropServices;

namespace CodexLidGuard;

internal enum LidState
{
    Unknown,
    Open,
    Closed
}

internal sealed class LidWatcher : IDisposable
{
    private const uint WmPowerBroadcast = 0x0218;
    private const uint WmClose = 0x0010;
    private const uint WmDestroy = 0x0002;
    private const int PbtPowerSettingChange = 0x8013;
    private const uint DeviceNotifyWindowHandle = 0;
    private static readonly Guid LidSwitchStateChange = new("ba3e0f4d-b817-4094-a2d1-d56379e6a0f3");

    private readonly Thread _thread;
    private readonly ManualResetEventSlim _ready = new();
    private readonly WndProc _windowProcedure;
    private IntPtr _window;
    private IntPtr _notification;
    private Exception? _startupError;
    private volatile LidState _state;
    private bool _disposed;

    public LidState State => _state;
    public event Action<LidState>? Changed;

    public LidWatcher()
    {
        _windowProcedure = WindowProcedure;
        _thread = new Thread(MessageLoop)
        {
            IsBackground = true,
            Name = "Codex Lid Guard lid-state listener"
        };
        _thread.Start();
        if (!_ready.Wait(TimeSpan.FromSeconds(3)))
        {
            throw new TimeoutException("Timed out while registering for Windows lid-state notifications.");
        }
        if (_startupError is not null)
        {
            throw new InvalidOperationException("Could not register for Windows lid-state notifications.", _startupError);
        }
    }

    private void MessageLoop()
    {
        var className = $"CodexLidGuardWindow.{Environment.ProcessId}";
        var instance = GetModuleHandle(null);
        var windowClass = new WindowClass
        {
            Size = (uint)Marshal.SizeOf<WindowClass>(),
            WindowProcedure = Marshal.GetFunctionPointerForDelegate(_windowProcedure),
            Instance = instance,
            ClassName = className
        };

        try
        {
            if (RegisterClassEx(ref windowClass) == 0)
            {
                throw new Win32Exception(Marshal.GetLastWin32Error(), "RegisterClassEx failed");
            }

            _window = CreateWindowEx(0, className, "Codex Lid Guard", 0,
                0, 0, 0, 0, new IntPtr(-3), IntPtr.Zero, instance, IntPtr.Zero);
            if (_window == IntPtr.Zero)
            {
                throw new Win32Exception(Marshal.GetLastWin32Error(), "CreateWindowEx failed");
            }

            var setting = LidSwitchStateChange;
            _notification = RegisterPowerSettingNotification(_window, ref setting, DeviceNotifyWindowHandle);
            if (_notification == IntPtr.Zero)
            {
                throw new Win32Exception(Marshal.GetLastWin32Error(), "RegisterPowerSettingNotification failed");
            }

            _ready.Set();
            while (GetMessage(out var message, IntPtr.Zero, 0, 0) > 0)
            {
                TranslateMessage(ref message);
                DispatchMessage(ref message);
            }
        }
        catch (Exception ex)
        {
            _startupError = ex;
            Log.Write($"Lid watcher failed: {ex.Message}");
            _ready.Set();
        }
        finally
        {
            if (_notification != IntPtr.Zero)
            {
                UnregisterPowerSettingNotification(_notification);
                _notification = IntPtr.Zero;
            }
            if (_window != IntPtr.Zero)
            {
                DestroyWindow(_window);
                _window = IntPtr.Zero;
            }
            UnregisterClass(className, instance);
        }
    }

    private IntPtr WindowProcedure(IntPtr window, uint message, IntPtr wParam, IntPtr lParam)
    {
        if (message == WmPowerBroadcast && wParam.ToInt64() == PbtPowerSettingChange && lParam != IntPtr.Zero)
        {
            var setting = Marshal.PtrToStructure<PowerBroadcastSetting>(lParam);
            if (setting.PowerSetting == LidSwitchStateChange && setting.DataLength >= sizeof(byte))
            {
                var value = Marshal.ReadByte(IntPtr.Add(lParam, Marshal.SizeOf<PowerBroadcastSetting>()));
                var next = value == 0 ? LidState.Closed : LidState.Open;
                if (_state != next)
                {
                    _state = next;
                    Log.Write($"Lid state changed to {next}.");
                    Changed?.Invoke(next);
                }
            }
            return IntPtr.Zero;
        }

        if (message == WmClose)
        {
            DestroyWindow(window);
            return IntPtr.Zero;
        }

        if (message == WmDestroy)
        {
            PostQuitMessage(0);
            return IntPtr.Zero;
        }

        return DefWindowProc(window, message, wParam, lParam);
    }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }
        _disposed = true;
        if (_window != IntPtr.Zero)
        {
            PostMessage(_window, WmClose, IntPtr.Zero, IntPtr.Zero);
        }
        _thread.Join(TimeSpan.FromSeconds(2));
        _ready.Dispose();
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct PowerBroadcastSetting
    {
        public Guid PowerSetting;
        public uint DataLength;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct WindowClass
    {
        public uint Size;
        public uint Style;
        public IntPtr WindowProcedure;
        public int ClassExtra;
        public int WindowExtra;
        public IntPtr Instance;
        public IntPtr Icon;
        public IntPtr Cursor;
        public IntPtr Background;
        [MarshalAs(UnmanagedType.LPWStr)] public string? MenuName;
        [MarshalAs(UnmanagedType.LPWStr)] public string? ClassName;
        public IntPtr SmallIcon;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct Message
    {
        public IntPtr Window;
        public uint Value;
        public IntPtr WParam;
        public IntPtr LParam;
        public uint Time;
        public Point Point;
        public uint Private;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct Point
    {
        public int X;
        public int Y;
    }

    private delegate IntPtr WndProc(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr GetModuleHandle(string? moduleName);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern ushort RegisterClassEx(ref WindowClass windowClass);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool UnregisterClass(string className, IntPtr instance);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateWindowEx(uint extendedStyle, string className, string windowName,
        uint style, int x, int y, int width, int height, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr parameter);

    [DllImport("user32.dll")]
    private static extern IntPtr DefWindowProc(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool DestroyWindow(IntPtr window);

    [DllImport("user32.dll")]
    private static extern int GetMessage(out Message message, IntPtr window, uint filterMin, uint filterMax);

    [DllImport("user32.dll")]
    private static extern bool TranslateMessage(ref Message message);

    [DllImport("user32.dll")]
    private static extern IntPtr DispatchMessage(ref Message message);

    [DllImport("user32.dll")]
    private static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern void PostQuitMessage(int exitCode);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern IntPtr RegisterPowerSettingNotification(IntPtr recipient, ref Guid powerSettingGuid, uint flags);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool UnregisterPowerSettingNotification(IntPtr handle);
}
