# Run by the AppImage's start script (AppRun) just before ShuttleSCP starts, after the
# GTK plugin's own hook (hooks run in name order; this one is installed as
# "zz-shuttlescp.sh" to come last).
#
# The GTK plugin forces the Adwaita theme and the X11 backend to play safe with every
# app. ShuttleSCP lets the user choose a theme (View > Theme) and works on Wayland, so
# both are undone here. Set APPIMAGE_GTK_THEME to force a theme anyway.
if [ -z "${APPIMAGE_GTK_THEME:-}" ]; then
    unset GTK_THEME
fi
unset GDK_BACKEND
