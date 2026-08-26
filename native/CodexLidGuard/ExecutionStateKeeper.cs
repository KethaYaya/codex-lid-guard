using System.Collections.Concurrent;
using System.Runtime.InteropServices;

namespace CodexLidGuard;

internal sealed class ExecutionStateKeeper : IDisposable
{
    private readonly BlockingCollection<Command> _commands = new();
    private readonly Thread _thread;
    private bool _disposed;

    public ExecutionStateKeeper()
    {
        _thread = new Thread(Run)
        {
            IsBackground = true,
            Name = "Codex Lid Guard execution-state keeper"
        };
        _thread.Start();
    }

    public void SetGuarding(bool enabled)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        using var acknowledged = new ManualResetEventSlim();
        _commands.Add(new Command(enabled, acknowledged));
        if (!acknowledged.Wait(TimeSpan.FromSeconds(2)))
        {
            throw new TimeoutException("Windows did not acknowledge the keep-awake request.");
        }
    }

    private void Run()
    {
        foreach (var command in _commands.GetConsumingEnumerable())
        {
            var flags = command.Enabled
                ? ExecutionState.Continuous | ExecutionState.SystemRequired
                : ExecutionState.Continuous;
            var result = SetThreadExecutionState(flags);
            if (result == 0)
            {
                Log.Write($"SetThreadExecutionState failed with Win32 error {Marshal.GetLastWin32Error()}.");
            }
            command.Acknowledged.Set();
        }

        SetThreadExecutionState(ExecutionState.Continuous);
    }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }

        _disposed = true;
        _commands.CompleteAdding();
        _thread.Join(TimeSpan.FromSeconds(2));
        _commands.Dispose();
    }

    private sealed record Command(bool Enabled, ManualResetEventSlim Acknowledged);

    [Flags]
    private enum ExecutionState : uint
    {
        SystemRequired = 0x00000001,
        Continuous = 0x80000000
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern ExecutionState SetThreadExecutionState(ExecutionState executionState);
}
