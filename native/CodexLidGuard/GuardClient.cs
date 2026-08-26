using System.Diagnostics;
using System.IO.Pipes;
using System.Text.Json;

namespace CodexLidGuard;

internal static class GuardClient
{
    public static async Task<GuardResponse> SendAsync(GuardRequest request, bool startDaemon = true)
    {
        var response = await TrySendAsync(request, 150);
        if (response is not null && !IsCompatibleDaemon(response) && startDaemon)
        {
            await RetireLegacyDaemonAsync();
            response = null;
        }
        if (response is not null || !startDaemon)
        {
            return response ?? new GuardResponse { Ok = false, Message = "The guardian is not running." };
        }

        StartDaemon();
        var deadline = DateTime.UtcNow.AddSeconds(4);
        do
        {
            await Task.Delay(100);
            response = await TrySendAsync(request, 400);
            if (response is not null && IsCompatibleDaemon(response))
            {
                return response;
            }
        } while (DateTime.UtcNow < deadline);

        return new GuardResponse { Ok = false, Message = "Could not start the guardian daemon." };
    }

    private static async Task<GuardResponse?> TrySendAsync(GuardRequest request, int timeoutMilliseconds)
    {
        try
        {
            await using var pipe = new NamedPipeClientStream(".", AppPaths.PipeName,
                PipeDirection.InOut, PipeOptions.Asynchronous);
            using var timeout = new CancellationTokenSource(timeoutMilliseconds);
            await pipe.ConnectAsync(timeout.Token);
            await using var writer = new StreamWriter(pipe, leaveOpen: true) { AutoFlush = true };
            using var reader = new StreamReader(pipe, leaveOpen: true);
            await writer.WriteLineAsync(JsonSerializer.Serialize(request, JsonDefaults.WireOptions));
            var line = await reader.ReadLineAsync(timeout.Token);
            return JsonSerializer.Deserialize<GuardResponse>(line ?? string.Empty, JsonDefaults.WireOptions);
        }
        catch (Exception ex) when (ex is TimeoutException or OperationCanceledException or IOException)
        {
            return null;
        }
    }

    private static void StartDaemon()
    {
        var executable = Environment.ProcessPath
            ?? throw new InvalidOperationException("Could not locate the guardian executable.");
        Process.Start(new ProcessStartInfo
        {
            FileName = executable,
            Arguments = "daemon",
            UseShellExecute = false,
            CreateNoWindow = true,
            WindowStyle = ProcessWindowStyle.Hidden
        });
    }

    private static async Task RetireLegacyDaemonAsync()
    {
        Log.Write("A legacy or different-build guardian daemon answered the pipe; restoring power policy and replacing it.");
        _ = await TrySendAsync(new GuardRequest { Action = "restore" }, 1_500);

        var currentProcessId = Environment.ProcessId;
        foreach (var process in Process.GetProcessesByName("CodexLidGuard"))
        {
            using (process)
            {
                if (process.Id == currentProcessId || process.HasExited)
                {
                    continue;
                }

                try
                {
                    process.Kill(entireProcessTree: false);
                    process.WaitForExit(2_000);
                    Log.Write($"Stopped legacy guardian process {process.Id}.");
                }
                catch (Exception ex) when (ex is InvalidOperationException or System.ComponentModel.Win32Exception or NotSupportedException)
                {
                    Log.Write($"Could not stop legacy guardian process {process.Id}: {ex.Message}");
                }
            }
        }

        await Task.Delay(100);
    }

    private static bool IsCompatibleDaemon(GuardResponse response)
    {
        if (response.ProtocolVersion != GuardProtocol.CurrentVersion ||
            string.IsNullOrWhiteSpace(response.DaemonPath) ||
            string.IsNullOrWhiteSpace(Environment.ProcessPath))
        {
            return false;
        }

        try
        {
            return string.Equals(
                Path.GetFullPath(response.DaemonPath),
                Path.GetFullPath(Environment.ProcessPath),
                StringComparison.OrdinalIgnoreCase);
        }
        catch
        {
            return false;
        }
    }
}
