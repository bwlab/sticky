project('sticky-notes', 'rust',
          version: '0.3.3',
    meson_version: '>= 0.62.0',
  default_options: [ 'warning_level=2', 'werror=false', ],
)

application_id = 'it.bwlab.StickyNotes'
GETTEXT_PACKAGE = application_id

i18n = import('i18n')
gnome = import('gnome')

cargo = find_program('cargo')

# A development build gets an id of its own, so it runs next to the stable one.
development = get_option('profile') == 'development'
app_id = development ? application_id + '.Devel' : application_id

cargo_build = custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  console: true,
  output: application_id,
  install: true,
  install_dir: get_option('bindir'),
  command: [
    meson.project_source_root() / 'build-aux/cargo-build.sh',
    meson.project_source_root(),
    meson.project_build_root() / 'target',
    development ? 'debug' : 'release',
    '@OUTPUT@',
    app_id,
    get_option('prefix') / get_option('localedir'),
  ],
)

subdir('data')
subdir('po')

gnome.post_install(
     glib_compile_schemas: true,
    gtk_update_icon_cache: true,
  update_desktop_database: true,
)
