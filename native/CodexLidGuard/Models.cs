using System.Text.Json.Serialization;
using System.Text.Json;

namespace CodexLidGuard;

internal static class GuardProtocol
{
    public const int CurrentVersion = 2;
}

internal sealed class GuardSettings
{
    [JsonPropertyName("alertSounds")]
    public bool AlertSounds { get; init; } = true;

    [JsonPropertyName("sleepWhenLidClosed")]
    public bool SleepWhenLidClosed { get; init; } = true;

    [JsonPropertyName("sleepDelaySeconds")]
    public int SleepDelaySeconds { get; init; } = 10;

    public static GuardSettings Load()
    {
        try
        {
            if (!File.Exists(AppPaths.SettingsFile))
            {
                return new GuardSettings();
            }

            var settings = JsonSerializer.Deserialize<GuardSettings>(File.ReadAllText(AppPaths.SettingsFile), JsonDefaults.Options)
                ?? new GuardSettings();
            return new GuardSettings
            {
                AlertSounds = settings.AlertSounds,
                SleepWhenLidClosed = settings.SleepWhenLidClosed,
                SleepDelaySeconds = Math.Clamp(settings.SleepDelaySeconds, 0, 300)
            };
        }
        catch (Exception ex)
        {
            Log.Write($"Could not read settings; using defaults. {ex.Message}");
            return new GuardSettings();
        }
    }
}

internal sealed class HookPayload
{
    [JsonPropertyName("session_id")]
    public string? SessionId { get; init; }

    [JsonPropertyName("turn_id")]
    public string? TurnId { get; init; }

    [JsonPropertyName("cwd")]
    public string? Cwd { get; init; }
}

internal sealed class GuardRequest
{
    [JsonPropertyName("action")]
    public string Action { get; init; } = string.Empty;

    [JsonPropertyName("sessionId")]
    public string? SessionId { get; init; }

    [JsonPropertyName("turnId")]
    public string? TurnId { get; init; }

    [JsonPropertyName("cwd")]
    public string? Cwd { get; init; }
}

internal sealed class GuardResponse
{
    [JsonPropertyName("protocolVersion")]
    public int ProtocolVersion { get; init; }

    [JsonPropertyName("daemonPath")]
    public string? DaemonPath { get; init; }

    [JsonPropertyName("ok")]
    public bool Ok { get; init; }

    [JsonPropertyName("message")]
    public string Message { get; init; } = string.Empty;

    [JsonPropertyName("activeTurns")]
    public int ActiveTurns { get; init; }

    [JsonPropertyName("isGuarding")]
    public bool IsGuarding { get; init; }

    [JsonPropertyName("lidState")]
    public string LidState { get; init; } = "unknown";

    [JsonPropertyName("sleepPending")]
    public bool SleepPending { get; init; }
}

internal sealed class SavedPowerState
{
    public Guid Scheme { get; init; }
    public uint AcLidAction { get; init; }
    public uint DcLidAction { get; init; }
    public DateTimeOffset CapturedAt { get; init; }
}

internal static class JsonDefaults
{
    public static readonly System.Text.Json.JsonSerializerOptions Options = new()
    {
        PropertyNameCaseInsensitive = true,
        WriteIndented = true
    };

    public static readonly System.Text.Json.JsonSerializerOptions WireOptions = new()
    {
        PropertyNameCaseInsensitive = true,
        WriteIndented = false
    };
}

internal static class Log
{
    private static readonly object Gate = new();

    public static void Write(string message)
    {
        try
        {
            Directory.CreateDirectory(AppPaths.DataDirectory);
            lock (Gate)
            {
                File.AppendAllText(AppPaths.LogFile, $"{DateTimeOffset.Now:O} {message}{Environment.NewLine}");
            }
        }
        catch
        {
            // Logging must never interfere with a Codex lifecycle hook.
        }
    }
}
