#!/usr/bin/env bash
# Makes Strata the Omarchy file manager. Usage: ./replace-files.sh [--undo] [--dry-run]
#   (default)  open folders, the "File manager" shortcuts and the menu's "Files" row in Strata
#   --undo     remove the shortcuts added here and restore the previous folder handler
#   --dry-run  print what would change without changing anything
set -euo pipefail

desktop_id=io.github.lgse.Strata.desktop
bindings="$HOME/.config/hypr/bindings.lua"
menu="$HOME/.config/omarchy/extensions/omarchy-menu.jsonc"
state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/strata"
previous_handler="$state_dir/previous-directory-handler"
begin='-- >>> strata replace-files managed block'
end='-- <<< strata replace-files managed block'
menu_begin='// >>> strata replace-files managed block'
menu_end='// <<< strata replace-files managed block'

undo=0
dry_run=0
for argument in "$@"; do
  case "$argument" in
    --undo) undo=1 ;;
    --dry-run) dry_run=1 ;;
    -h|--help)
      sed -n '2,5p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "Unknown option: $argument" >&2
      exit 2
      ;;
  esac
done

run() {
  if (( dry_run )); then
    printf 'would run:'
    printf ' %q' "$@"
    printf '\n'
  else
    "$@"
  fi
}

strip_block() {
  local file="$1" begin="$2" end="$3"
  [[ -f $file ]] || return 0
  local temporary
  temporary="$(mktemp)"
  awk -v begin="$begin" -v end="$end" '
    { line = $0; sub(/^[ \t]+/, "", line) }
    line == begin { skipping = 1; next }
    line == end { skipping = 0; next }
    !skipping { print }
  ' "$file" > "$temporary"
  if (( dry_run )); then
    echo "would remove the managed block from $file"
    rm -f "$temporary"
  else
    cat "$temporary" > "$file"
    rm -f "$temporary"
  fi
}

# Inserts the Files row directly after the opening brace of the menu extension.
add_menu_row() {
  local row="  $menu_begin
  \"files\": {\"icon\":\"󰉋\",\"label\":\"Files\",\"aliases\":[\"files\",\"file manager\",\"strata\"],\"checked\":\"xdg-mime query default inode/directory | grep -qx $desktop_id\",\"action\":\"uwsm-app -- $binary\"},
  $menu_end"
  if (( dry_run )); then
    printf 'would add to %s:\n%s\n' "$menu" "$row"
    return 0
  fi
  local temporary
  temporary="$(mktemp)"
  awk -v row="$row" 'NR == 1 { print; print row; next } { print }' "$menu" > "$temporary"
  cat "$temporary" > "$menu"
  rm -f "$temporary"
}

reload_hyprland() {
  if command -v hyprctl >/dev/null 2>&1 && [[ -n ${HYPRLAND_INSTANCE_SIGNATURE:-} ]]; then
    run hyprctl reload
    (( dry_run )) || hyprctl configerrors || true
  fi
}

if (( undo )); then
  strip_block "$bindings" "$begin" "$end"
  strip_block "$menu" "$menu_begin" "$menu_end"
  if [[ -f $previous_handler ]]; then
    run xdg-mime default "$(cat "$previous_handler")" inode/directory
    (( dry_run )) || rm -f "$previous_handler"
  fi
  reload_hyprland
  echo "Restored the previous file manager settings."
  exit 0
fi

binary="$HOME/.local/bin/strata"
if [[ ! -x $binary ]]; then
  echo "Strata is not installed at $binary. Run ./build.sh first." >&2
  exit 1
fi

applications="$HOME/.local/share/applications"
if [[ ! -f $applications/$desktop_id ]]; then
  source_entry="$(dirname "${BASH_SOURCE[0]}")/data/$desktop_id"
  [[ -f $source_entry ]] || { echo "Missing $source_entry" >&2; exit 1; }
  run install -d "$applications"
  if (( dry_run )); then
    echo "would install $applications/$desktop_id with Exec=$binary"
  else
    sed "s|^Exec=strata |Exec=$binary |" "$source_entry" > "$applications/$desktop_id"
  fi
  run update-desktop-database "$applications"
fi

current="$(xdg-mime query default inode/directory || true)"
if [[ -n $current && $current != "$desktop_id" ]]; then
  run mkdir -p "$state_dir"
  if (( dry_run )); then
    echo "would remember $current as the previous folder handler"
  else
    printf '%s\n' "$current" > "$previous_handler"
  fi
fi
run xdg-mime default "$desktop_id" inode/directory

if [[ -f $bindings ]]; then
  backup="$bindings.bak.$(date +%s)"
  run cp "$bindings" "$backup"
  strip_block "$bindings" "$begin" "$end"
  block="$begin
-- Open Omarchy's file manager shortcuts and folders in Strata.
hl.unbind(\"SUPER + SHIFT + F\")
hl.unbind(\"SUPER + ALT + SHIFT + F\")
o.bind(\"SUPER + SHIFT + F\", \"File manager\", { launch = \"$binary\" })
o.bind(\"SUPER + ALT + SHIFT + F\", \"File manager (cwd)\",
  \"uwsm-app -- $binary \\\"\$(omarchy-cmd-terminal-cwd)\\\"\")
$end"
  if (( dry_run )); then
    printf 'would append to %s:\n%s\n' "$bindings" "$block"
  else
    printf '\n%s\n' "$block" >> "$bindings"
  fi
  reload_hyprland
else
  echo "No $bindings found; only the folder handler was changed." >&2
fi

if [[ -f $menu ]]; then
  run cp "$menu" "$menu.bak.$(date +%s)"
  strip_block "$menu" "$menu_begin" "$menu_end"
  if [[ $(head -n 1 "$menu") == "{" ]]; then
    add_menu_row
  else
    echo "Unexpected layout in $menu; the Files menu row was not added." >&2
  fi
else
  echo "No $menu found; the Files menu row was not added." >&2
fi

echo "Strata now opens folders, the File manager shortcuts and the Files menu row."
echo "Undo with: ./replace-files.sh --undo"
