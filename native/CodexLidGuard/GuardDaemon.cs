using System.IO.Pipes;
using System.Text.Json;

namespace CodexLidGuard;

internal sealed class GuardDaemon : IDisposable
{
    private readonly HashSet<string> _activeTurns = new(StringComparer.Ordinal);
    private readonly WindowsPowerPolicy _powerPolicy;
    private readonly LidWatcher _lidWatcher;
    private readonly object _gate = new();
    private CancellationTokenSource? _pendingSleep;
    private bool _disposed;

    public GuardDaemon()
    {
        _powerPolicy = new WindowsPowerPolicy();
        _lidWatcher = new LidWatcher();
        _lidWatcher.Changed += OnLidChanged;
    }

    public async Task RunAsync(CancellationToken cancellationToken)
    {
        Log.Write($"Guardian daemon started on pipe {AppPaths.PipeName}.");
        while (!cancellationToken.IsCancellationRequested)
        {
            await using var pipe = new NamedPipeServerStream(
                AppPaths.PipeName,
                PipeDirection.InOut,
                1,
                PipeTransmissionMode.Byte,
                PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
            try
            {
                using var idleCancellation = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
                if (CanExitWhenIdle())
                {
                    idleCancellation.CancelAfter(TimeSpan.FromMinutes(5));
                }
                await pipe.WaitForConnectionAsync(idleCancellation.Token);
                await HandleConnectionAsync(pipe, cancellationToken);
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                break;
            }
            catch (OperationCanceledException) when (CanExitWhenIdle())
            {
                Log.Write("Guardian daemon reached its idle timeout.");
                break;
            }
            catch (IOException ex)
            {
                Log.Write($"Pipe connection ended unexpectedly: {ex.Message}");
            }
            catch (Exception ex)
            {
                Log.Write($"Request handling failed: {ex}");
            }
        }
    }

    private bool CanExitWhenIdle()
    {
        lock (_gate)
        {
            return _activeTurns.Count == 0 && _pendingSleep is null;
        }
    }

    private async Task HandleConnectionAsync(Stream pipe, CancellationToken cancellationToken)
    {
        using var reader = new StreamReader(pipe, leaveOpen: true);
        await using var writer = new StreamWriter(pipe, leaveOpen: true) { AutoFlush = true };
        var line = await reader.ReadLineAsync(cancellationToken);
        GuardResponse response;
        try
        {
            var request = JsonSerializer.Deserialize<GuardRequest>(line ?? string.Empty, JsonDefaults.WireOptions)
                ?? throw new InvalidDataException("The guardian request was empty.");
            response = Handle(request);
        }
        catch (Exception ex)
        {
            Log.Write($"Guardian request failed: {ex.Message}");
            response = Snapshot(false, ex.Message);
        }
        await writer.WriteLineAsync(JsonSerializer.Serialize(response, JsonDefaults.WireOptions));
    }

    private GuardResponse Handle(GuardRequest request)
    {
        lock (_gate)
        {
            return request.Action.ToLowerInvariant() switch
            {
                "acquire" => Acquire(request),
                "release" => Release(request),
                "release-session" => ReleaseSession(request),
                "restore" => Restore(),
                "status" => Snapshot(true, "Status read."),
                _ => Snapshot(false, $"Unknown guardian action '{request.Action}'.")
            };
        }
    }

    private GuardResponse Acquire(GuardRequest request)
    {
        var key = TurnKey(request);
        var sessionPrefix = SessionPrefix(request);
        CancelPendingSleep();
        var replacedTurns = _activeTurns.RemoveWhere(candidate =>
            candidate.StartsWith(sessionPrefix, StringComparison.Ordinal) &&
            !candidate.Equals(key, StringComparison.Ordinal));
        if (replacedTurns > 0)
        {
            Log.Write($"Removed {replacedTurns} stale turn(s) for session {sessionPrefix[..^1]}.");
        }
        if (_activeTurns.Add(key) && _activeTurns.Count == 1)
        {
            try
            {
                _powerPolicy.Acquire();
            }
            catch
            {
                _activeTurns.Remove(key);
                throw;
            }
        }
        Log.Write($"Turn acquired: {key}. Active turns: {_activeTurns.Count}.");
        return Snapshot(true, "Windows will stay awake until the Codex turn finishes.");
    }

    private GuardResponse Release(GuardRequest request)
    {
        var key = TurnKey(request);
        _activeTurns.Remove(key);
        Log.Write($"Turn released: {key}. Active turns: {_activeTurns.Count}.");
        FinishIfIdle();
        return Snapshot(true, "Codex turn finished.");
    }

    private GuardResponse ReleaseSession(GuardRequest request)
    {
        var session = request.SessionId ?? string.Empty;
        _activeTurns.RemoveWhere(key => key.StartsWith(session + ":", StringComparison.Ordinal));
        Log.Write($"Session released: {session}. Active turns: {_activeTurns.Count}.");
        FinishIfIdle();
        return Snapshot(true, "Codex session finished.");
    }

    private GuardResponse Restore()
    {
        _activeTurns.Clear();
        CancelPendingSleep();
        _powerPolicy.Release();
        return Snapshot(true, "The original Windows power policy was restored.");
    }

    private void FinishIfIdle()
    {
        if (_activeTurns.Count != 0)
        {
            return;
        }

        _powerPolicy.Release();
        ScheduleSleepIfNeeded();
    }

    private void ScheduleSleepIfNeeded()
    {
        var settings = GuardSettings.Load();
        if (!settings.SleepWhenLidClosed || _lidWatcher.State != LidState.Closed)
        {
            return;
        }

        CancelPendingSleep();
        _pendingSleep = new CancellationTokenSource();
        var token = _pendingSleep.Token;
        Log.Write($"Sleep scheduled in {settings.SleepDelaySeconds} seconds because the lid is closed.");
        _ = Task.Run(async () =>
        {
            try
            {
                await Task.Delay(TimeSpan.FromSeconds(settings.SleepDelaySeconds), token);
                lock (_gate)
                {
                    if (!token.IsCancellationRequested && _activeTurns.Count == 0 && _lidWatcher.State == LidState.Closed)
                    {
                        if (!WindowsPowerPolicy.Suspend())
                        {
                            Log.Write("Windows rejected the sleep request.");
                        }
                        _pendingSleep?.Dispose();
                        _pendingSleep = null;
                    }
                }
            }
            catch (OperationCanceledException)
            {
                // A new turn or an open lid intentionally cancels the pending sleep.
            }
            catch (Exception ex)
            {
                Log.Write($"Sleep request failed: {ex.Message}");
            }
        }, token);
    }

    private void OnLidChanged(LidState state)
    {
        lock (_gate)
        {
            if (state == LidState.Open)
            {
                CancelPendingSleep();
            }
            else if (state == LidState.Closed && _activeTurns.Count == 0)
            {
                // Closing the lid while no Codex turn is active must retain normal Windows
                // behavior. We only schedule explicit sleep after a guarded turn releases.
            }
        }
    }

    private void CancelPendingSleep()
    {
        _pendingSleep?.Cancel();
        _pendingSleep?.Dispose();
        _pendingSleep = null;
    }

    private GuardResponse Snapshot(bool ok, string message) => new()
    {
        ProtocolVersion = GuardProtocol.CurrentVersion,
        DaemonPath = Environment.ProcessPath,
        Ok = ok,
        Message = message,
        ActiveTurns = _activeTurns.Count,
        IsGuarding = _powerPolicy.IsGuarding,
        LidState = _lidWatcher.State.ToString().ToLowerInvariant(),
        SleepPending = _pendingSleep is { IsCancellationRequested: false }
    };

    private static string TurnKey(GuardRequest request)
    {
        var session = SessionPrefix(request)[..^1];
        var turn = string.IsNullOrWhiteSpace(request.TurnId) ? "current-turn" : request.TurnId;
        return $"{session}:{turn}";
    }

    private static string SessionPrefix(GuardRequest request)
    {
        var session = string.IsNullOrWhiteSpace(request.SessionId) ? "unknown-session" : request.SessionId;
        return $"{session}:";
    }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }

        _disposed = true;
        lock (_gate)
        {
            CancelPendingSleep();
            _activeTurns.Clear();
            _powerPolicy.Dispose();
            _lidWatcher.Dispose();
        }
        Log.Write("Guardian daemon stopped.");
    }
}
