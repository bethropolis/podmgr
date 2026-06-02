#!/usr/bin/env bash
# podmgr — uninstall script
# Removes binaries, completions, and optionally config/data/quadlet files.
set -euo pipefail

# ── Terminal capability detection ────────────────────────
_ncolors=$(tput colors 2>/dev/null || echo 0)
_has_unicode=true
if [[ "${LANG:-}" != *"UTF-8"* && "${LC_ALL:-}" != *"UTF-8"* ]]; then
  _has_unicode=false
fi

# ── Colors ───────────────────────────────────────────────
RST=$'\033[0m'
BOLD=$'\033[1m'
DIM=$'\033[2m'
ITAL=$'\033[3m'
RED=$'\033[38;5;203m'
GREEN=$'\033[38;5;114m'
YELLOW=$'\033[38;5;221m'
BLUE=$'\033[38;5;75m'
CYAN=$'\033[38;5;87m'
PURPLE=$'\033[38;5;141m'
GRAY=$'\033[38;5;245m'
WHITE=$'\033[38;5;255m'
BG_DARK=$'\033[48;5;234m'

# True-color gradient stops (blue → violet → fuchsia)
TC0=$'\033[38;2;59;130;246m'
TC1=$'\033[38;2;99;102;241m'
TC2=$'\033[38;2;139;92;246m'
TC3=$'\033[38;2;168;85;247m'
TC4=$'\033[38;2;217;70;239m'
TC5=$'\033[38;2;236;72;153m'

# ── Unicode symbols with ASCII fallback ──────────────────
if $_has_unicode; then
  SYM_OK="✓";    SYM_WARN="⚠";  SYM_ERR="✗"
  SYM_ARR="›";   SYM_DOT="·";   SYM_DASH="─"
  SYM_TL="╭";    SYM_BL="╰";    SYM_V="│"
  SYM_BULLET="▸"
else
  SYM_OK="[ok]"; SYM_WARN="[!]"; SYM_ERR="[x]"
  SYM_ARR=">";   SYM_DOT=".";    SYM_DASH="-"
  SYM_TL="+";    SYM_BL="+";     SYM_V="|"
  SYM_BULLET=">"
fi

# ── Logging ──────────────────────────────────────────────
_width() { tput cols 2>/dev/null || echo 80; }

info()  { printf "  ${GRAY}${SYM_DOT}${RST}  %s\n" "$*"; }
ok()    { printf "  ${GREEN}${SYM_OK}${RST}  %s\n" "$*"; }
warn()  { printf "  ${YELLOW}${SYM_WARN}${RST}  ${YELLOW}%s${RST}\n" "$*" >&2; }
die()   { printf "\n  ${RED}${SYM_ERR}${RST}  ${RED}%s${RST}\n\n" "$*" >&2; exit 1; }
detail(){ printf "     ${DIM}%s${RST}\n" "$*"; }

step() {
  local label="$*"
  printf "\n  ${PURPLE}${SYM_BULLET}${RST}  ${BOLD}${WHITE}%s${RST}\n" "$label"
  local w; w=$(( $(_width) - 6 ))
  local line
  printf -v line '%*s' "$w" ''
  printf "     ${DIM}${GRAY}%s${RST}\n" "${line// /${SYM_DASH}}"
}

# ── Horizontal rule ──────────────────────────────────────
hr() {
  local w; w=$(( $(_width) - 4 ))
  local line
  printf -v line '%*s' "$w" ''
  printf "  ${DIM}${GRAY}%s${RST}\n" "${line// /${SYM_DASH}}"
}

asroot() { if [ -n "${SUDO:-}" ]; then command sudo "$@"; else "$@"; fi }

# ── Banner ───────────────────────────────────────────────
banner() {
  printf "\n"
  printf "  ${TC0} ▒▒███                                               ${RST}\n"
  printf "  ${TC1}████████   ██████   ███████  █████████████    ███████ ████████${RST}\n"
  printf "  ${TC2}▒▒███▒▒███ ███▒▒███ ███▒▒███ ▒▒███▒▒███▒▒███  ███▒▒███▒▒███▒▒███${RST}\n"
  printf "  ${TC2} ▒███ ▒███▒███ ▒███▒███ ▒███  ▒███ ▒███ ▒███ ▒███ ▒███ ▒███ ▒▒▒ ${RST}\n"
  printf "  ${TC3} ▒███ ▒███▒███ ▒███▒███ ▒███  ▒███ ▒███ ▒███ ▒███ ▒███ ▒███     ${RST}\n"
  printf "  ${TC3} ▒███████ ▒▒██████ ▒▒████████ █████▒███ █████▒▒███████ █████    ${RST}\n"
  printf "  ${TC4} ▒███▒▒▒   ▒▒▒▒▒▒   ▒▒▒▒▒▒▒▒ ▒▒▒▒▒ ▒▒▒ ▒▒▒▒▒  ▒▒▒▒▒███▒▒▒▒▒     ${RST}\n"
  printf "  ${TC4} ▒███                                         ███ ▒███          ${RST}\n"
  printf "  ${TC5} █████                                       ▒▒██████           ${RST}\n"
  printf "  ${TC5} ▒▒▒▒▒                                         ▒▒▒▒▒▒           ${RST}\n"
  printf "\n"
  printf "  ${DIM}${GRAY}Podman-native container environment manager — uninstall${RST}\n"
  printf "\n"
}

# ── Paths ────────────────────────────────────────────────
SYSTEM=false
PREFIX="${PREFIX:-$HOME/.local}"
SUDO=""
BIN_DIR="$PREFIX/bin"
COMP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/bash-completion/completions"
FISH_COMP_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/fish/completions"

PODMGR_CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/podmgr"
PODMGR_DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/podmgr"
PODMGR_ICONS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons/podmgr"
PODMGR_APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
PODMGR_STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/podmgr"

# ── Parse flags ──────────────────────────────────────────
REMOVE_CONFIG=false
REMOVE_DATA=false
REMOVE_QUADLET=false
FORCE=false

usage() {
  printf "\n  ${BOLD}Usage:${RST} %s [options]\n\n" "$0"
  printf "  ${GRAY}Options:${RST}\n"
  printf "    ${CYAN}--system${RST}      Uninstall from /usr/local ${DIM}(requires sudo)${RST}\n"
  printf "    ${CYAN}--config${RST}      Remove config files ${DIM}(~/.config/podmgr)${RST}\n"
  printf "    ${CYAN}--data${RST}        Remove data files ${DIM}(~/.local/share/podmgr, icons, .desktop)${RST}\n"
  printf "    ${CYAN}--quadlet${RST}     Disable and remove Quadlet systemd files\n"
  printf "    ${CYAN}--all${RST}         Remove everything ${DIM}(config + data + quadlet)${RST}\n"
  printf "    ${CYAN}--force${RST}       Skip confirmation prompts\n"
  printf "    ${CYAN}--help${RST}        Show this help\n\n"
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --system)   SYSTEM=true ;;
    --config)   REMOVE_CONFIG=true ;;
    --data)     REMOVE_DATA=true ;;
    --quadlet)  REMOVE_QUADLET=true ;;
    --all)      REMOVE_CONFIG=true; REMOVE_DATA=true; REMOVE_QUADLET=true ;;
    --force)    FORCE=true ;;
    --help|-h)  usage ;;
    *)          printf "  ${RED}Unknown option:${RST} %s\n" "$1"; usage ;;
  esac
  shift
done

if $SYSTEM; then
  PREFIX="/usr/local"
  BIN_DIR="/usr/local/bin"
  COMP_DIR="/usr/share/bash-completion/completions"
  FISH_COMP_DIR="/usr/share/fish/completions"
  SUDO="sudo"
fi

# ── Confirmation ─────────────────────────────────────────
confirm() {
  if $FORCE; then return 0; fi
  printf "\n  ${YELLOW}${SYM_WARN}${RST}  ${BOLD}%s${RST} ${DIM}[y/N]${RST} " "$*"
  read -r ans
  [[ "$ans" == "y" || "$ans" == "Y" ]]
}

# ── Remove binaries ──────────────────────────────────────
remove_binaries() {
  step "Removing binaries"
  local removed=false
  for bin in podmgr podmgr-guest; do
    if [ -f "$BIN_DIR/$bin" ]; then
      asroot rm -f "$BIN_DIR/$bin"
      ok "$bin  ${DIM}← $BIN_DIR/$bin${RST}"
      removed=true
    else
      info "$bin  ${DIM}not found${RST}"
    fi
  done
  $removed || info "No binaries to remove"
}

# ── Remove completions ───────────────────────────────────
remove_completions() {
  step "Removing shell completions"

  if [ -f "$COMP_DIR/podmgr" ]; then
    asroot rm -f "$COMP_DIR/podmgr"
    ok "bash  ${DIM}← $COMP_DIR/podmgr${RST}"
  fi
  if [ -f "$COMP_DIR/_podmgr" ]; then
    asroot rm -f "$COMP_DIR/_podmgr"
    ok "zsh  ${DIM}← $COMP_DIR/_podmgr${RST}"
  fi
  if [ -f "$FISH_COMP_DIR/podmgr.fish" ]; then
    asroot rm -f "$FISH_COMP_DIR/podmgr.fish"
    ok "fish  ${DIM}← $FISH_COMP_DIR/podmgr.fish${RST}"
  fi
}

# ── Remove config ────────────────────────────────────────
remove_config() {
  step "Removing config"
  if [ -d "$PODMGR_CONFIG_DIR" ]; then
    if confirm "Remove $PODMGR_CONFIG_DIR?"; then
      rm -rf "$PODMGR_CONFIG_DIR"
      ok "Removed $PODMGR_CONFIG_DIR"
    else
      info "Skipped"
    fi
  else
    info "No config directory found"
  fi
}

# ── Remove data ──────────────────────────────────────────
remove_data() {
  step "Removing data"
  local removed=false

  if [ -d "$PODMGR_DATA_DIR" ]; then
    if confirm "Remove $PODMGR_DATA_DIR?"; then
      rm -rf "$PODMGR_DATA_DIR"
      ok "Removed $PODMGR_DATA_DIR"
      removed=true
    else
      info "Skipped"
    fi
  fi

  if [ -d "$PODMGR_ICONS_DIR" ]; then
    rm -rf "$PODMGR_ICONS_DIR"
    ok "Removed $PODMGR_ICONS_DIR"
    removed=true
  fi

  if [ -d "$PODMGR_APPS_DIR" ]; then
    local count=0
    for f in "$PODMGR_APPS_DIR"/podmgr-*.desktop; do
      [ -f "$f" ] || continue
      rm -f "$f"
      count=$((count + 1))
    done
    if [ "$count" -gt 0 ]; then
      ok "Removed $count exported .desktop files"
      removed=true
    fi
  fi

  if [ -d "$PODMGR_STATE_DIR" ]; then
    rm -rf "$PODMGR_STATE_DIR"
    ok "Removed $PODMGR_STATE_DIR"
    removed=true
  fi

  $removed || info "No data files to remove"
}

# ── Remove Quadlet ───────────────────────────────────────
remove_quadlet() {
  step "Disabling Quadlet files"
  if command -v podmgr &>/dev/null; then
    if confirm "Disable and remove Quadlet systemd files (podmgr disable)?"; then
      podmgr disable 2>/dev/null && ok "Quadlet files disabled" || warn "podmgr disable failed"
    else
      info "Skipped"
    fi
  else
    local qdir="${XDG_CONFIG_HOME:-$HOME/.config}/containers/systemd"
    if [ -d "$qdir" ]; then
      local count=0
      for f in "$qdir"/podmgr-*.{container,service,socket,build}; do
        [ -f "$f" ] || continue
        rm -f "$f"
        count=$((count + 1))
      done
      if [ "$count" -gt 0 ]; then
        ok "Removed $count Quadlet files from $qdir"
        systemctl --user daemon-reload 2>/dev/null || true
        return
      fi
    fi
    info "No Quadlet files found"
  fi
}

# ── Summary ──────────────────────────────────────────────
print_summary() {
  printf "\n"
  hr
  printf "\n"
  printf "  ${GREEN}${SYM_OK}${RST}  ${BOLD}${WHITE}Uninstall complete${RST}\n\n"

  if ! $REMOVE_CONFIG && ! $REMOVE_DATA && ! $REMOVE_QUADLET; then
    printf "  ${GRAY}${SYM_V}${RST}\n"
    printf "  ${GRAY}${SYM_V}${RST}  ${GRAY}Tips:${RST}\n"
    printf "  ${GRAY}${SYM_V}${RST}  ${SYM_ARR}  ${CYAN}--all${RST}    ${DIM}also remove config, data, and quadlet files${RST}\n"
    printf "  ${GRAY}${SYM_V}${RST}  ${SYM_ARR}  ${CYAN}--help${RST}   ${DIM}see all options${RST}\n"
    printf "  ${GRAY}${SYM_BL}${RST}\n"
  else
    printf "  ${GRAY}${SYM_V}${RST}\n"
    printf "  ${GRAY}${SYM_V}${RST}  ${GRAY}Cleanup tip:${RST}\n"
    printf "  ${GRAY}${SYM_V}${RST}  ${SYM_ARR}  ${CYAN}podman rmi localhost/podmgr-*${RST}  ${DIM}remove leftover images${RST}\n"
    printf "  ${GRAY}${SYM_BL}${RST}\n"
  fi
  printf "\n"
}

# ── Main ─────────────────────────────────────────────────
main() {
  banner
  hr

  # Mode header
  local mode_label
  if $SYSTEM; then
    mode_label="${YELLOW}system-wide${RST}  ${DIM}(requires sudo)${RST}"
  else
    mode_label="${CYAN}user${RST}  ${DIM}(${PREFIX})${RST}"
  fi
  printf "\n"
  printf "  ${GRAY}mode    ${RST}%b\n" "$mode_label"
  printf "  ${GRAY}prefix  ${RST}${WHITE}%s${RST}\n" "$PREFIX"
  printf "  ${GRAY}bin     ${RST}${WHITE}%s${RST}\n" "$BIN_DIR"

  local opts=()
  $REMOVE_CONFIG && opts+=("config")
  $REMOVE_DATA && opts+=("data")
  $REMOVE_QUADLET && opts+=("quadlet")
  $FORCE && opts+=("force")

  if [ ${#opts[@]} -gt 0 ]; then
    printf "  ${GRAY}flags   ${RST}${WHITE}%s${RST}\n" "${opts[*]}"
  else
    printf "  ${GRAY}flags   ${RST}${DIM}(binaries + completions only)${RST}\n"
  fi
  printf "\n"
  hr

  remove_binaries
  remove_completions

  if $REMOVE_QUADLET; then remove_quadlet; fi
  if $REMOVE_CONFIG; then remove_config; fi
  if $REMOVE_DATA; then remove_data; fi

  print_summary
}

main "$@"
