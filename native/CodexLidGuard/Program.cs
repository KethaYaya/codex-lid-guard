using System.Diagnostics;
using System.Text.Json;

namespace CodexLidGuard;

internal static class Program
{
    public static async Task<int> Main(string[] args)
    {
        if (!OperatingSystem.IsWindows())
        {
            Console.Error.WriteLine("Codex Lid Guard supports Windows only.");
            return 1;
        }

        try
        {
            var command = args.FirstOrDefault()?.ToLowerInvariant() ?? "status";
            return command switch
            {
                "daemon" => await RunDaemonAsync(),
                "hook" => await RunHookAsync(args.Skip(1).FirstOrDefault() ?? string.Empty),
                "sound" => RunSound(args.Skip(1).FirstOrDefault() ?? string.Empty, writeResponse: true),
                "play-sound" => RunSound(args.Skip(1).FirstOrDefault() ?? string.Empty, writeResponse: false),
                "status" => await RunControlAsync("status"),
                "restore" => await RunControlAsync("restore"),
                _ => Usage()
            };
        }
        catch (Exception ex)
        {
            Log.Write($"Fatal command failure: {ex}");
            Console.Error.WriteLine(ex.Message);
            return 1;
        }
    }

    private static async Task<int> RunDaemonAsync()
    {
        using var mutex = new Mutex(true, AppPaths.MutexName, out var ownsMutex);
        if (!ownsMutex)
        {
            return 0;
        }

        using var cancellation = new CancellationTokenSource();
        Console.CancelKeyPress += (_, eventArgs) =>
        {
            eventArgs.Cancel = true;
            cancellation.Cancel();
        };
        AppDomain.CurrentDomain.ProcessExit += (_, _) => cancellation.Cancel();

        using var daemon = new GuardDaemon();
        await daemon.RunAsync(cancellation.Token);
        return 0;
    }

    private static async Task<int> RunHookAsync(string action)
    {
        HookPayload payload;
        try
        {
            var input = await Console.In.ReadToEndAsync();
            payload = JsonSerializer.Deserialize<HookPayload>(input, JsonDefaults.Options) ?? new HookPayload();
        }
        catch (JsonException ex)
        {
            Log.Write($"Hook input was not valid JSON: {ex.Message}");
            payload = new HookPayload();
        }

        if (action.Equals("sound-request", StringComparison.OrdinalIgnoreCase))
        {
            StartSoundPlayer(AlertSound.Request);
            Console.WriteLine("{\"continue\":true}");
            return 0;
        }

        var response = await GuardClient.SendAsync(new GuardRequest
        {
            Action = action,
            SessionId = payload.SessionId,
            TurnId = payload.TurnId,
            Cwd = payload.Cwd
        });

        if (!response.Ok)
        {
            Log.Write($"Hook '{action}' could not update the guardian: {response.Message}");
        }
        else if (action.Equals("release", StringComparison.OrdinalIgnoreCase))
        {
            StartSoundPlayer(AlertSound.Done);
        }

        // This is valid for both UserPromptSubmit and Stop and never blocks or extends the turn.
        Console.WriteLine("{\"continue\":true}");
        return 0;
    }

    private static async Task<int> RunControlAsync(string action)
    {
        var response = await GuardClient.SendAsync(new GuardRequest { Action = action });
        Console.WriteLine(JsonSerializer.Serialize(response, JsonDefaults.Options));
        return response.Ok ? 0 : 1;
    }

    private static int RunSound(string sound, bool writeResponse)
    {
        var alertSound = sound.ToLowerInvariant() switch
        {
            "done" => AlertSound.Done,
            "request" => AlertSound.Request,
            _ => (AlertSound?)null
        };
        if (alertSound is null)
        {
            return Usage();
        }

        var played = AlertSoundPlayer.PlayAndWait(alertSound.Value);
        if (writeResponse)
        {
            var label = alertSound == AlertSound.Done ? "completion" : "needs-response";
            Console.WriteLine(JsonSerializer.Serialize(new GuardResponse
            {
                ProtocolVersion = GuardProtocol.CurrentVersion,
                DaemonPath = Environment.ProcessPath,
                Ok = played,
                Message = played ? $"Played the {label} alert." : $"Could not play the {label} alert."
            }, JsonDefaults.Options));
        }
        return played ? 0 : 1;
    }

    private static void StartSoundPlayer(AlertSound sound)
    {
        var executable = Environment.ProcessPath;
        if (string.IsNullOrWhiteSpace(executable))
        {
            Log.Write("Could not locate the alert sound player executable.");
            return;
        }

        var startInfo = new ProcessStartInfo
        {
            FileName = executable,
            UseShellExecute = false,
            CreateNoWindow = true,
            WindowStyle = ProcessWindowStyle.Hidden,
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true
        };
        startInfo.ArgumentList.Add("play-sound");
        startInfo.ArgumentList.Add(sound == AlertSound.Done ? "done" : "request");
        Process.Start(startInfo)?.Dispose();
    }

    private static int Usage()
    {
        Console.Error.WriteLine("Usage: CodexLidGuard [daemon | hook acquire | hook release | hook release-session | sound done | sound request | status | restore]");
        return 2;
    }
}
