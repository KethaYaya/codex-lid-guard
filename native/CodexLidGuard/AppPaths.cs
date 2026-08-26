using System.Security.Principal;
using System.Text;

namespace CodexLidGuard;

internal static class AppPaths
{
    public static string DataDirectory { get; } = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "CodexLidGuard");

    public static string SettingsFile => Path.Combine(DataDirectory, "settings.json");
    public static string RecoveryFile => Path.Combine(DataDirectory, "power-recovery.json");
    public static string LogFile => Path.Combine(DataDirectory, "guard.log");

    public static string PipeName { get; } = $"CodexLidGuard.{SessionKey()}";
    public static string MutexName { get; } = $"Local\\CodexLidGuard.{SessionKey()}";

    private static string SessionKey()
    {
        string identity;
        try
        {
            identity = WindowsIdentity.GetCurrent().User?.Value ?? Environment.UserName;
        }
        catch
        {
            identity = Environment.UserName;
        }

        var stable = $"{identity}:{System.Diagnostics.Process.GetCurrentProcess().SessionId}";
        var bytes = System.Security.Cryptography.SHA256.HashData(Encoding.UTF8.GetBytes(stable));
        return Convert.ToHexString(bytes.AsSpan(0, 8));
    }
}
