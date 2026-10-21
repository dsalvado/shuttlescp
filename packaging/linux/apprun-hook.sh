# Run by the AppImage's start script (AppRun) just before ShuttleSCP starts, after the
# GTK plugin's own hook (hooks run in name order; this one is installed as
# "zz-shuttlescp.sh" to come last).
#
# The GTK plugin forces the Adwaita theme and the X11 backend to play safe with every
# app. ShuttleSCP lets the user choose a theme (View > Theme) and works on Wayland, so
# both are undone here.
#
# The plugin's hook sets APPIMAGE_GTK_THEME itself, to "Adwaita:light" or
# "Adwaita:dark", when the user did not; only another value is the user's own choice
# (set APPIMAGE_GTK_THEME to force a theme anyway). packaging/linux/check-appimage.sh
# checks the result on every built AppImage.
case "${APPIMAGE_GTK_THEME:-}" in
    "" | "Adwaita:light" | "Adwaita:dark") unset GTK_THEME ;;
esac
unset GDK_BACKEND
