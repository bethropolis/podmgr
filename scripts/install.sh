#!/usr/bin/env bash
# podbox — install script
# Install to ~/.local/bin or $PREFIX/bin
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

asroot() { if [ -n "${SUDO:-}" ]; then command sudo "$@"; else "$@"; fi }

# ── Banner ───────────────────────────────────────────────
banner() {
  printf "\n"
  printf "  ${TC0}                        █████ █████                         ${RST}\n"
  printf "  ${TC1}                       ▒▒███ ▒▒███                          ${RST}\n"
  printf "  ${TC2} ████████   ██████   ███████  ▒███████   ██████  █████ █████${RST}\n"
  printf "  ${TC2}▒▒███▒▒███ ███▒▒███ ███▒▒███  ▒███▒▒███ ███▒▒███▒▒███ ▒▒███ ${RST}\n"
  printf "  ${TC3} ▒███ ▒███▒███ ▒███▒███ ▒███  ▒███ ▒███▒███ ▒███ ▒▒▒█████▒  ${RST}\n"
  printf "  ${TC3} ▒███ ▒███▒███ ▒███▒███ ▒███  ▒███ ▒███▒███ ▒███  ███▒▒▒███ ${RST}\n"
  printf "  ${TC4} ▒███████ ▒▒██████ ▒▒████████ ████████ ▒▒██████  █████ █████${RST}\n"
  printf "  ${TC4} ▒███▒▒▒   ▒▒▒▒▒▒   ▒▒▒▒▒▒▒▒ ▒▒▒▒▒▒▒▒   ▒▒▒▒▒▒  ▒▒▒▒▒ ▒▒▒▒▒ ${RST}\n"
  printf "  ${TC5} ▒███                                                       ${RST}\n"
  printf "  ${TC5} █████                                                      ${RST}\n"
  printf "  ${TC5}▒▒▒▒▒                                                       ${RST}\n"
  printf "\n"
  printf "  ${DIM}${GRAY}Podman-native container environment manager${RST}\n"
  printf "\n"
}

# ── Horizontal rule ──────────────────────────────────────
hr() {
  local w; w=$(( $(_width) - 4 ))
  local line
  printf -v line '%*s' "$w" ''
  printf "  ${DIM}${GRAY}%s${RST}\n" "${line// /${SYM_DASH}}"
}

# ── Detect distro ────────────────────────────────────────
detect_distro() {
  if   command -v pacman  &>/dev/null; then echo "arch"
  elif command -v apt-get &>/dev/null; then echo "debian"
  elif command -v dnf     &>/dev/null; then echo "fedora"
  else echo "other"
  fi
}

# ── Prerequisites ────────────────────────────────────────
check_prereqs() {
  local missing=()
  command -v cargo  &>/dev/null || missing+=("cargo  ${DIM}(install via rustup.rs)${RST}")
  command -v podman &>/dev/null && ok "podman found" || warn "podman not found — required at runtime"

  if [ ${#missing[@]} -gt 0 ]; then
    for m in "${missing[@]}"; do
      printf "  ${RED}${SYM_ERR}${RST}  Missing: %b\n" "$m"
    done
    exit 1
  fi
  ok "cargo found"
}

# ── Defaults ─────────────────────────────────────────────
SYSTEM=false
PREFIX="${PREFIX:-$HOME/.local}"
SUDO=""
BIN_DIR="$PREFIX/bin"
COMP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/bash-completion/completions"
ZSH_COMP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/zsh/site-functions"
FISH_COMP_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/fish/completions"

usage() {
  printf "\n  ${BOLD}Usage:${RST} %s [options]\n\n" "$0"
  printf "  ${GRAY}Options:${RST}\n"
  printf "    ${CYAN}--system${RST}       Install system-wide to /usr/local ${DIM}(requires sudo)${RST}\n"
  printf "    ${CYAN}--skip-build${RST}   Use existing binaries without rebuilding\n"
  printf "    ${CYAN}--help${RST}         Show this help\n\n"
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --system)     SYSTEM=true ;;
    --skip-build) PODBOX_SKIP_BUILD=1 ;;
    --help|-h)    usage ;;
    *)            printf "  ${RED}Unknown option:${RST} %s\n" "$1"; usage ;;
  esac
  shift
done

if $SYSTEM; then
  PREFIX="/usr/local"
  BIN_DIR="/usr/local/bin"
  COMP_DIR="/usr/share/bash-completion/completions"
  ZSH_COMP_DIR="/usr/share/zsh/site-functions"
  FISH_COMP_DIR="/usr/share/fish/completions"
  SUDO="sudo"
fi

# ── Build ─────────────────────────────────────────────────
build_binaries() {
  if [ -n "${PODBOX_SKIP_BUILD:-}" ]; then
    info "Skipping build ${DIM}(PODBOX_SKIP_BUILD is set)${RST}"
    return
  fi

  step "Building podbox"

  printf "     ${GRAY}cargo build --release -p podbox${RST}\n"
  if cargo build --release -p podbox 2>&1 | \
      grep -E "^(error|warning\[)" | \
      while IFS= read -r line; do detail "$line"; done; [ "${PIPESTATUS[0]}" -eq 0 ]; then
    ok "podbox"
  else
    cargo build --release -p podbox || die "podbox build failed"
    ok "podbox"
  fi
}

# ── Install binaries ──────────────────────────────────────
install_binaries() {
  step "Installing binary"

  asroot mkdir -p "$BIN_DIR"

  local podbox_src="${PODBOX_BIN:-$PWD/target/release/podbox}"

  [ -f "$podbox_src" ]  || die "Binary not found: $podbox_src  (hint: set PODBOX_BIN)"

  asroot install -m 755 "$podbox_src" "$BIN_DIR/podbox"
  ok "podbox  ${DIM}→ $BIN_DIR/podbox${RST}"
}

# ── Completions ───────────────────────────────────────────
install_completions() {
  step "Installing shell completions"

  asroot mkdir -p "$COMP_DIR" "$ZSH_COMP_DIR" "$FISH_COMP_DIR"

  if "$BIN_DIR/podbox" completions bash 2>/dev/null | asroot tee "$COMP_DIR/podbox" >/dev/null; then
    ok "bash          ${DIM}→ $COMP_DIR/podbox${RST}"
  else
    warn "bash completions skipped"
  fi

  if "$BIN_DIR/podbox" completions zsh 2>/dev/null | asroot tee "$ZSH_COMP_DIR/_podbox" >/dev/null; then
    ok "zsh           ${DIM}→ $ZSH_COMP_DIR/_podbox${RST}"
  else
    warn "zsh completions skipped"
  fi

  if "$BIN_DIR/podbox" completions fish 2>/dev/null | asroot tee "$FISH_COMP_DIR/podbox.fish" >/dev/null; then
    ok "fish          ${DIM}→ $FISH_COMP_DIR/podbox.fish${RST}"
  else
    warn "fish completions skipped"
  fi
}

# ── Summary ────────────────────────────────────────────────
print_summary() {
  local distro; distro=$(detect_distro)

  printf "\n"
  hr
  printf "\n"
  printf "  ${GREEN}${SYM_OK}${RST}  ${BOLD}${WHITE}Installation complete${RST}\n\n"

  # PATH warning if needed
  if ! $SYSTEM && [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    printf "  ${YELLOW}${SYM_WARN}${RST}  ${YELLOW}%s${RST} is not in your PATH\n" "$BIN_DIR"
    printf "\n"
    printf "     Add to your shell config:\n\n"
    printf "     ${BG_DARK}  ${CYAN}export PATH=\"\$PATH:%s\"${RST}${BG_DARK}  ${RST}\n" "$BIN_DIR"
    printf "\n"
  fi

  printf "  ${GRAY}${SYM_V}${RST}\n"
  printf "  ${GRAY}${SYM_V}${RST}  ${GRAY}Get started:${RST}\n"
  printf "  ${GRAY}${SYM_V}${RST}\n"
  printf "  ${GRAY}${SYM_V}${RST}  ${SYM_ARR}  ${CYAN}podbox doctor${RST}   ${DIM}verify the installation${RST}\n"
  printf "  ${GRAY}${SYM_V}${RST}  ${SYM_ARR}  ${CYAN}podbox init${RST}     ${DIM}create your first container${RST}\n"
  printf "  ${GRAY}${SYM_BL}${RST}\n"
  printf "\n"
}

# ── Main ──────────────────────────────────────────────────
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
  printf "\n"
  hr

  step "Checking prerequisites"
  check_prereqs

  build_binaries
  install_binaries
  install_completions

  print_summary
}

main "$@"
