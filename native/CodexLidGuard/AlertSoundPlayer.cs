using System.Diagnostics;

namespace CodexLidGuard;

internal enum AlertSound
{
    Done,
    Request
}

internal static class AlertSoundPlayer
{
    private const string SoundPathEnvironmentVariable = "CODEX_LID_GUARD_SOUND_PATH";
    private const string PlayerScript = """
        $ErrorActionPreference = 'Stop'
        $Path = [Environment]::GetEnvironmentVariable('CODEX_LID_GUARD_SOUND_PATH', 'Process')
        if ([string]::IsNullOrWhiteSpace($Path)) { throw 'CODEX_LID_GUARD_SOUND_PATH is not set' }
        Add-Type -AssemblyName PresentationCore
        Add-Type -AssemblyName WindowsBase
        $resolved = (Resolve-Path -LiteralPath $Path).ProviderPath
        $script:player = [System.Windows.Media.MediaPlayer]::new()
        $script:frame = [System.Windows.Threading.DispatcherFrame]::new()
        $script:timer = [System.Windows.Threading.DispatcherTimer]::new()
        $script:timer.Interval = [TimeSpan]::FromSeconds(15)
        $script:failed = $null
        $script:timedOut = $false
        $script:player.add_MediaOpened({ $script:player.Play() })
        $script:player.add_MediaEnded({ $script:frame.Continue = $false })
        $script:player.add_MediaFailed({
            param($sender, $eventArgs)
            $script:failed = $eventArgs.ErrorException
            $script:frame.Continue = $false
        })
        $script:timer.add_Tick({
            $script:timedOut = $true
            $script:frame.Continue = $false
        })
        try {
            $script:player.Open([Uri]::new($resolved))
            $script:timer.Start()
            [System.Windows.Threading.Dispatcher]::PushFrame($script:frame)
        } finally {
            $script:timer.Stop()
            $script:player.Close()
        }
        if ($script:failed) { throw "sound media failed: $($script:failed.Message)" }
        if ($script:timedOut) { throw 'sound playback timed out' }
        """;

    public static bool PlayAndWait(AlertSound sound)
    {
        if (!GuardSettings.Load().AlertSounds)
        {
            return true;
        }

        var fileName = sound == AlertSound.Done ? "done.mp3" : "request.mp3";
        var soundPath = Path.Combine(AppContext.BaseDirectory, "sounds", fileName);
        if (!File.Exists(soundPath))
        {
            Log.Write($"Alert sound is missing: {soundPath}");
            return false;
        }

        var startInfo = new ProcessStartInfo
        {
            FileName = "powershell.exe",
            UseShellExecute = false,
            CreateNoWindow = true,
            WindowStyle = ProcessWindowStyle.Hidden,
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true
        };
        startInfo.ArgumentList.Add("-NoLogo");
        startInfo.ArgumentList.Add("-NoProfile");
        startInfo.ArgumentList.Add("-NonInteractive");
        startInfo.ArgumentList.Add("-ExecutionPolicy");
        startInfo.ArgumentList.Add("Bypass");
        startInfo.ArgumentList.Add("-Command");
        startInfo.ArgumentList.Add(PlayerScript);
        startInfo.Environment[SoundPathEnvironmentVariable] = soundPath;

        try
        {
            using var player = Process.Start(startInfo)
                ?? throw new InvalidOperationException("Windows did not start the sound player.");
            var standardOutput = player.StandardOutput.ReadToEndAsync();
            var standardError = player.StandardError.ReadToEndAsync();
            if (!player.WaitForExit(20_000))
            {
                player.Kill(entireProcessTree: true);
                player.WaitForExit();
                Log.Write($"Could not play {sound.ToString().ToLowerInvariant()} alert sound: playback timed out.");
                return false;
            }

            _ = standardOutput.GetAwaiter().GetResult();
            var error = standardError.GetAwaiter().GetResult().Trim();
            if (player.ExitCode != 0)
            {
                Log.Write($"Could not play {sound.ToString().ToLowerInvariant()} alert sound: {error}.");
                return false;
            }

            Log.Write($"Played {sound.ToString().ToLowerInvariant()} alert sound.");
            return true;
        }
        catch (Exception ex)
        {
            Log.Write($"Could not play {sound.ToString().ToLowerInvariant()} alert sound: {ex.Message}.");
            return false;
        }
    }
}
