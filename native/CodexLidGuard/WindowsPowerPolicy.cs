using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text.Json;

namespace CodexLidGuard;

internal sealed class WindowsPowerPolicy : IDisposable
{
    // The Windows power APIs predate in-parameters and require writable GUID refs.
    private static Guid PowerButtonsSubgroup = new("4f971e89-eebd-4455-a8de-9e59040e7347");
    private static Guid LidCloseAction = new("5ca83367-6e45-459f-a27b-476b1d01c936");
    private readonly ExecutionStateKeeper _executionState = new();
    private SavedPowerState? _saved;
    private bool _guarding;
    private bool _disposed;

    public bool IsGuarding => _guarding;

    public WindowsPowerPolicy()
    {
        RestoreStaleState();
    }

    public void Acquire()
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        if (_guarding)
        {
            return;
        }

        var scheme = GetActiveScheme();
        Check(PowerReadACValueIndex(IntPtr.Zero, ref scheme, ref PowerButtonsSubgroup, ref LidCloseAction, out var ac),
            "read the AC lid-close action");
        Check(PowerReadDCValueIndex(IntPtr.Zero, ref scheme, ref PowerButtonsSubgroup, ref LidCloseAction, out var dc),
            "read the battery lid-close action");

        _saved = new SavedPowerState
        {
            Scheme = scheme,
            AcLidAction = ac,
            DcLidAction = dc,
            CapturedAt = DateTimeOffset.UtcNow
        };
        SaveRecovery(_saved);

        try
        {
            // Index 0 is the documented "Do nothing" lid action. Both AC and DC are
            // protected so closing the lid cannot interrupt a running local turn.
            Check(PowerWriteACValueIndex(IntPtr.Zero, ref scheme, ref PowerButtonsSubgroup, ref LidCloseAction, 0),
                "set the AC lid-close action");
            Check(PowerWriteDCValueIndex(IntPtr.Zero, ref scheme, ref PowerButtonsSubgroup, ref LidCloseAction, 0),
                "set the battery lid-close action");
            Check(PowerSetActiveScheme(IntPtr.Zero, ref scheme), "activate the temporary power policy");
            _executionState.SetGuarding(true);
            _guarding = true;
            Log.Write($"Guard acquired for power scheme {scheme}; original lid actions AC={ac}, DC={dc}.");
        }
        catch
        {
            TryRestore(_saved);
            throw;
        }
    }

    public void Release()
    {
        if (!_guarding && _saved is null && !File.Exists(AppPaths.RecoveryFile))
        {
            return;
        }

        _executionState.SetGuarding(false);
        var state = _saved ?? LoadRecovery();
        if (state is not null)
        {
            Restore(state);
            Log.Write($"Restored power scheme {state.Scheme} lid actions AC={state.AcLidAction}, DC={state.DcLidAction}.");
        }

        _saved = null;
        _guarding = false;
    }

    public static bool Suspend()
    {
        Log.Write("Requesting Windows sleep after the Codex task completed with the lid closed.");
        return SetSuspendState(false, false, false);
    }

    private void RestoreStaleState()
    {
        var stale = LoadRecovery();
        if (stale is null)
        {
            return;
        }

        Log.Write("Found an interrupted guard session; restoring its saved lid policy.");
        TryRestore(stale);
    }

    private static void SaveRecovery(SavedPowerState state)
    {
        Directory.CreateDirectory(AppPaths.DataDirectory);
        var temporary = AppPaths.RecoveryFile + ".tmp";
        File.WriteAllText(temporary, JsonSerializer.Serialize(state, JsonDefaults.Options));
        File.Move(temporary, AppPaths.RecoveryFile, true);
    }

    private static SavedPowerState? LoadRecovery()
    {
        try
        {
            return File.Exists(AppPaths.RecoveryFile)
                ? JsonSerializer.Deserialize<SavedPowerState>(File.ReadAllText(AppPaths.RecoveryFile), JsonDefaults.Options)
                : null;
        }
        catch (Exception ex)
        {
            Log.Write($"Could not read the recovery state: {ex.Message}");
            return null;
        }
    }

    private static void TryRestore(SavedPowerState? state)
    {
        if (state is null)
        {
            return;
        }

        try
        {
            Restore(state);
        }
        catch (Exception ex)
        {
            // Keep the recovery file so a later invocation can retry safely.
            Log.Write($"Could not restore the saved power policy: {ex.Message}");
        }
    }

    private static void Restore(SavedPowerState state)
    {
        var scheme = state.Scheme;
        Check(PowerWriteACValueIndex(IntPtr.Zero, ref scheme, ref PowerButtonsSubgroup, ref LidCloseAction, state.AcLidAction),
            "restore the AC lid-close action");
        Check(PowerWriteDCValueIndex(IntPtr.Zero, ref scheme, ref PowerButtonsSubgroup, ref LidCloseAction, state.DcLidAction),
            "restore the battery lid-close action");
        Check(PowerSetActiveScheme(IntPtr.Zero, ref scheme), "reactivate the restored power policy");
        File.Delete(AppPaths.RecoveryFile);
    }

    private static Guid GetActiveScheme()
    {
        Check(PowerGetActiveScheme(IntPtr.Zero, out var pointer), "read the active power scheme");
        try
        {
            return Marshal.PtrToStructure<Guid>(pointer);
        }
        finally
        {
            LocalFree(pointer);
        }
    }

    private static void Check(uint result, string operation)
    {
        if (result != 0)
        {
            throw new Win32Exception((int)result, $"Could not {operation}");
        }
    }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }

        try
        {
            Release();
        }
        catch (Exception ex)
        {
            Log.Write($"Power-policy cleanup failed: {ex.Message}");
        }

        _executionState.Dispose();
        _disposed = true;
    }

    [DllImport("powrprof.dll")]
    private static extern uint PowerGetActiveScheme(IntPtr userRootPowerKey, out IntPtr activePolicyGuid);

    [DllImport("powrprof.dll")]
    private static extern uint PowerReadACValueIndex(IntPtr rootPowerKey, ref Guid schemeGuid,
        ref Guid subgroupOfPowerSettingsGuid, ref Guid powerSettingGuid, out uint acValueIndex);

    [DllImport("powrprof.dll")]
    private static extern uint PowerReadDCValueIndex(IntPtr rootPowerKey, ref Guid schemeGuid,
        ref Guid subgroupOfPowerSettingsGuid, ref Guid powerSettingGuid, out uint dcValueIndex);

    [DllImport("powrprof.dll")]
    private static extern uint PowerWriteACValueIndex(IntPtr rootPowerKey, ref Guid schemeGuid,
        ref Guid subgroupOfPowerSettingsGuid, ref Guid powerSettingGuid, uint acValueIndex);

    [DllImport("powrprof.dll")]
    private static extern uint PowerWriteDCValueIndex(IntPtr rootPowerKey, ref Guid schemeGuid,
        ref Guid subgroupOfPowerSettingsGuid, ref Guid powerSettingGuid, uint dcValueIndex);

    [DllImport("powrprof.dll")]
    private static extern uint PowerSetActiveScheme(IntPtr userRootPowerKey, ref Guid schemeGuid);

    [DllImport("powrprof.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetSuspendState([MarshalAs(UnmanagedType.Bool)] bool hibernate,
        [MarshalAs(UnmanagedType.Bool)] bool forceCritical,
        [MarshalAs(UnmanagedType.Bool)] bool disableWakeEvent);

    [DllImport("kernel32.dll")]
    private static extern IntPtr LocalFree(IntPtr memory);
}
