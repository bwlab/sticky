application_id = 'it.bwlab.StickyNotes'

scalable_dir = join_paths('hicolor', 'scalable', 'apps')
# The icons are named after the app id: that is how the tray icon and the
# launcher find them, and how Flatpak decides to export them.
install_data(
  join_paths(scalable_dir, ('@0@.svg').format(application_id)),
  rename: ('@0@.svg').format(app_id),
  install_dir: join_paths(get_option('datadir'), 'icons', scalable_dir)
)

symbolic_dir = join_paths('hicolor', 'symbolic', 'apps')
install_data(
  join_paths(symbolic_dir, ('@0@-symbolic.svg').format(application_id)),
  rename: ('@0@-symbolic.svg').format(app_id),
  install_dir: join_paths(get_option('datadir'), 'icons', symbolic_dir)
)
