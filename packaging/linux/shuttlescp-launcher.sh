#!/bin/sh
# Starts ShuttleSCP inside the AppImage (installed as usr/bin/shuttlescp; the program
# itself is usr/bin/shuttlescp.bin). See build-appimage.sh.
#
# The program was built with glibc @GLIBC@. On a system with that version or newer it
# runs as usual. On an older one it runs with the glibc, C++ runtime and font libraries
# carried in the AppImage's compat folder, started through that glibc's own loader.
# Those libraries find each other, and the AppImage's usual ones in usr/lib, through
# the loader's --library-path. It applies to ShuttleSCP alone: programs it starts (a
# browser, a file's default app) still use the system's libraries.

here=$(dirname "$(readlink -f "$0")")
bundled=@GLIBC@
system=$(getconf GNU_LIBC_VERSION 2>/dev/null | awk '{print $2}')

# The older of the two versions, by version order (2.9 < 2.35).
oldest=$(printf '%s\n%s\n' "$system" "$bundled" | sort -V | head -n 1)

if [ -n "$system" ] && [ "$system" != "$bundled" ] && [ "$oldest" = "$system" ]; then
    compat="$here/../../compat"
    exec "$compat/ld-linux-x86-64.so.2" --library-path "$compat:$here/../lib" --argv0 "$0" \
        "$here/shuttlescp.bin" "$@"
fi
exec "$here/shuttlescp.bin" "$@"
