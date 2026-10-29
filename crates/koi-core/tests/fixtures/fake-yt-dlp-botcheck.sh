#!/bin/sh
# Fake yt-dlp that always runs into YouTube's bot check. Used by tests and for
# manual checks: set it as the yt-dlp path in koi-music's Settings.
for arg in "$@"; do
    if [ "$arg" = "--version" ]; then
        echo "2026.01.01"
        exit 0
    fi
done
for arg in "$@"; do
    if [ "$arg" = "-v" ]; then
        # Debug header of `yt-dlp -v` without a URL, as used by koi-music's probe.
        echo "[debug] JS runtimes: deno-2.0.0" >&2
        echo "yt-dlp: error: You must provide at least one URL." >&2
        exit 2
    fi
done
echo "ERROR: [youtube] abcdefghijk: Sign in to confirm you’re not a bot. Use --cookies-from-browser or --cookies for the authentication." >&2
exit 1
