#!/bin/sh
# install.sh: `curl -fsSL https://raw.githubusercontent.com/kendricklawton/boxdesk/main/install.sh | sh`
#
# Installs the release `cargo xtask dist` built for this host. macOS on ARM64 gets
# /Applications/Boxdesk.app and /usr/local/bin/boxdesk pointing into it; Linux on x86_64 gets
# bin/boxdesk, bin/Boxdesk and share/... under the first of /usr/local, /usr, / whose bin is on
# PATH. Both then unpack the guest tree to where `boxdesk` looks for one, and offer the package
# manager's libkrun, which no release can carry.
#
# The body is one function called on the last line, so a truncated download runs nothing.
#
# Environment:
#   BOXDESK_VERSION=0.0.5       that tag's assets rather than the latest release
#   BOXDESK_INSTALL_YES=1       run the libkrun package-manager line without asking (also --yes)
#   BOXDESK_NO_START=1          do not open the app afterwards (macOS)
#   BOXDESK_REPLACE_ROOTFS=1    replace a guest tree this script did not write
#   BOXDESK_INSTALL_DRY_RUN=1   print each command that would change this machine, run none (also --dry-run)

set -eu

REPO="kendricklawton/boxdesk"
APP="Boxdesk"
APP_DIR="/Applications/$APP.app"
CLI_IN_APP="Contents/Resources/boxdesk"
ROOTFS_IN_APP="Contents/Resources/rootfs.tar.gz"
# The release binary names libkrun by the install name Homebrew gave it and loads libkrunfw from
# this prefix's lib, so a boot needs these two files, not "libkrun somewhere".
BREW_PREFIX="/opt/homebrew"
BREW_LINE="brew tap slp/krun && brew trust slp/krun && brew install libkrun libkrunfw"

DRY_RUN="${BOXDESK_INSTALL_DRY_RUN:-}"
YES="${BOXDESK_INSTALL_YES:-}"
MISSING_LIBKRUN=
SUDO=

status() { echo ">>> $*" >&2; }
warning() { echo "WARNING: $*" >&2; }
error() { echo "ERROR: $*" >&2; exit 1; }
available() { command -v "$1" >/dev/null 2>&1; }

# Every command that changes this machine goes through here, so a dry run lists them and runs none.
run() {
    if [ -n "$DRY_RUN" ]; then
        echo "+ $*" >&2
        return 0
    fi
    "$@"
}

# As this user first, then under sudo: /usr/local/bin and /Applications are root's on some hosts.
as_admin() {
    if [ -n "$DRY_RUN" ]; then
        run "$@"
        return 0
    fi
    if "$@" 2>/dev/null; then
        return 0
    fi
    status "This step needs administrator rights (sudo may ask for your password)"
    sudo "$@"
}

# Under sudo, as the person who ran it: the guest tree is theirs, not root's.
as_user() {
    if [ "$(id -u)" -eq 0 ] && [ -n "${SUDO_USER:-}" ]; then
        run sudo -u "$SUDO_USER" -H "$@"
    else
        run "$@"
    fi
}

as_root() {
    if [ -n "$SUDO" ]; then
        run sudo "$@"
    else
        run "$@"
    fi
}

require() {
    missing=
    for tool in "$@"; do
        available "$tool" || missing="$missing $tool"
    done
    [ -z "$missing" ] && return 0
    if [ -n "$DRY_RUN" ]; then
        warning "not installed here:$missing"
        return 0
    fi
    error "these tools are needed and missing:$missing"
}

# Asks on the terminal: stdin is the script itself when it arrives through a pipe. A "no" and a
# missing terminal both answer no, and the caller prints the command for the person to run.
confirm() {
    [ -n "$YES" ] && return 0
    if ! { : </dev/tty; } 2>/dev/null; then
        return 1
    fi
    printf '%s [y/N] ' "$1" >&2
    read -r answer </dev/tty
    case "$answer" in
        [Yy] | [Yy][Ee][Ss]) return 0 ;;
        *) return 1 ;;
    esac
}

sha256_of() {
    case "$OS" in
        Darwin) shasum -a 256 "$1" ;;
        *) sha256sum "$1" ;;
    esac | cut -d' ' -f1
}

fetch() {
    status "Downloading $BASE_URL/$1"
    curl --fail --show-error --location --progress-bar -o "$TEMP_DIR/$1" "$BASE_URL/$1"
}

# SHA256SUMS lists bare asset names in `sha256sum` text form: two spaces, no `*`, no directory.
verify() {
    expected=$(awk -v name="$1" '$2 == name { print $1 }' "$TEMP_DIR/SHA256SUMS")
    [ -n "$expected" ] || error "SHA256SUMS has no line for $1"
    actual=$(sha256_of "$TEMP_DIR/$1")
    [ "$actual" = "$expected" ] || error "sha256 mismatch for $1: expected $expected, got $actual"
    status "sha256 verified for $1"
}

obtain() {
    if [ -n "$DRY_RUN" ]; then
        echo "+ curl $BASE_URL/SHA256SUMS $BASE_URL/$ASSET (then verify the sha256 and unpack)" >&2
        return 0
    fi
    fetch SHA256SUMS
    fetch "$ASSET"
    verify "$ASSET"
}

# Where `boxdesk` looks for a guest tree when no flag names one, in its own order:
# $BOXDESK_GUEST_ROOT, else $XDG_DATA_HOME/boxdesk/rootfs, else ~/.local/share/boxdesk/rootfs.
# Under sudo the person's home is read from passwd, and XDG_DATA_HOME is root's, so it is not used.
guest_root() {
    if [ -n "${BOXDESK_GUEST_ROOT:-}" ]; then
        echo "$BOXDESK_GUEST_ROOT"
        return
    fi
    if [ "$(id -u)" -eq 0 ] && [ -n "${SUDO_USER:-}" ]; then
        home=$(getent passwd "$SUDO_USER" 2>/dev/null | cut -d: -f6)
        echo "${home:-/home/$SUDO_USER}/.local/share/boxdesk/rootfs"
        return
    fi
    echo "${XDG_DATA_HOME:-$HOME/.local/share}/boxdesk/rootfs"
}

# Unpacks the guest tree the release carries. rootfs.sha256 beside the tree records which archive
# wrote it: the same archive is skipped, a different one replaces the tree, and a tree with no
# record (`cargo xtask init`, or a hand-made one) is kept unless BOXDESK_REPLACE_ROOTFS says so.
install_rootfs() {
    archive="$1"
    root=$(guest_root)
    record="$(dirname "$root")/rootfs.sha256"
    if [ -f "$archive" ]; then
        sum=$(sha256_of "$archive")
    else
        sum="(the sha256 of $archive)"
    fi
    if [ -f "$record" ] && [ "$(cat "$record")" = "$sum" ]; then
        status "The guest tree at $root is already this release's"
        return 0
    fi
    if [ -d "$root" ] && [ -n "$(ls -A "$root")" ]; then
        if [ ! -f "$record" ] && [ -z "${BOXDESK_REPLACE_ROOTFS:-}" ]; then
            warning "$root holds a guest tree this installer did not write; keeping it (BOXDESK_REPLACE_ROOTFS=1 replaces it)"
            return 0
        fi
        if [ ! -d "$root/bin" ] || [ ! -d "$root/usr" ]; then
            error "$root holds something that is not a guest tree (no bin and usr), so it is not removed; move it yourself"
        fi
        status "Replacing the guest tree at $root"
        as_user rm -rf "$root"
    fi
    status "Unpacking the guest tree to $root"
    as_user mkdir -p "$root"
    as_user tar -xzf "$archive" -C "$root"
    printf '%s\n' "$sum" | as_user tee "$record" >/dev/null
}

# Offers the package-manager line for libkrun and libkrunfw: a C library and a shared object
# holding a Linux kernel, which no release of this project can carry.
offer_libkrun() {
    label="$1"
    tool="$2"
    line="$3"
    if [ -n "$DRY_RUN" ]; then
        echo "+ $line (after asking; libkrun was not found here)" >&2
        return 0
    fi
    if ! available "$tool"; then
        warning "libkrun is not installed and neither is $tool. Install $label, then run:"
        echo "    $line" >&2
        MISSING_LIBKRUN=1
        return 0
    fi
    if confirm "Install libkrun and libkrunfw with: $line ?"; then
        sh -c "$line"
        return 0
    fi
    warning "libkrun was not installed. A sandbox will not boot until you run:"
    echo "    $line" >&2
    MISSING_LIBKRUN=1
}

ensure_libkrun_macos() {
    if [ -e "$BREW_PREFIX/opt/libkrun/lib/libkrun.1.dylib" ] && [ -e "$BREW_PREFIX/lib/libkrunfw.5.dylib" ]; then
        status "libkrun and libkrunfw found under $BREW_PREFIX"
        return 0
    fi
    offer_libkrun "Homebrew" brew "$BREW_LINE"
}

libkrun_linked() {
    for ldconfig in ldconfig /sbin/ldconfig /usr/sbin/ldconfig; do
        if available "$ldconfig" && "$ldconfig" -p 2>/dev/null | grep -q 'libkrun\.so\.1'; then
            return 0
        fi
    done
    for dir in /usr/lib64 /usr/lib /usr/local/lib; do
        [ -e "$dir/libkrun.so.1" ] && return 0
    done
    return 1
}

# Asks which package manager is here, never which distro this is. Debian and Ubuntu carry no
# libkrun package, so there is nothing to offer them but the upstream repository.
ensure_libkrun_linux() {
    if libkrun_linked; then
        status "libkrun found (libkrun.so.1)"
        return 0
    fi
    sudo_prefix="${SUDO:+$SUDO }"
    if available pacman; then
        offer_libkrun pacman pacman "${sudo_prefix}pacman -S --needed libkrun libkrunfw"
    elif available dnf; then
        offer_libkrun dnf dnf "${sudo_prefix}dnf install -y libkrun libkrunfw"
    elif available zypper; then
        offer_libkrun zypper zypper "${sudo_prefix}zypper install -y libkrun libkrunfw"
    else
        [ -n "$DRY_RUN" ] && return 0
        warning "no package manager here carries libkrun (pacman, dnf and zypper do; apt does not)."
        warning "Build it from https://github.com/containers/libkrun; a sandbox will not boot until libkrun.so.1 loads."
        MISSING_LIBKRUN=1
    fi
}

kvm_usable() {
    if [ "$(id -u)" -eq 0 ] && [ -n "${SUDO_USER:-}" ]; then
        sudo -u "$SUDO_USER" sh -c '[ -r /dev/kvm ] && [ -w /dev/kvm ]'
    else
        [ -r /dev/kvm ] && [ -w /dev/kvm ]
    fi
}

stop_app() {
    if pgrep -x "$APP" >/dev/null 2>&1; then
        status "Stopping the running $APP"
        run pkill -x "$APP"
    fi
}

install_macos() {
    require curl unzip shasum tar
    obtain
    if [ -z "$DRY_RUN" ]; then
        status "Unpacking $ASSET"
        unzip -q "$TEMP_DIR/$ASSET" -d "$TEMP_DIR"
        [ -d "$TEMP_DIR/$APP.app" ] || error "$ASSET does not contain $APP.app"
    fi
    stop_app
    if [ -d "$APP_DIR" ]; then
        status "Replacing $APP_DIR"
        as_admin rm -rf "$APP_DIR"
    fi
    status "Installing $APP_DIR"
    as_admin mv "$TEMP_DIR/$APP.app" "$APP_DIR"

    target="$APP_DIR/$CLI_IN_APP"
    if [ "$(readlink /usr/local/bin/boxdesk 2>/dev/null || true)" != "$target" ]; then
        status "Adding 'boxdesk' to PATH as /usr/local/bin/boxdesk"
        as_admin mkdir -p /usr/local/bin
        as_admin ln -sf "$target" /usr/local/bin/boxdesk
    fi

    install_rootfs "$APP_DIR/$ROOTFS_IN_APP"
    ensure_libkrun_macos

    if [ "$(sysctl -n kern.hv_support 2>/dev/null || echo 0)" != 1 ]; then
        warning "kern.hv_support is not 1: Hypervisor.framework will not start a sandbox on this machine"
    fi
    if [ -z "${BOXDESK_NO_START:-}" ]; then
        run open -a "$APP"
    fi
    status "Install complete. Run 'boxdesk' from the command line, or open $APP."
}

install_linux() {
    if [ "$(id -u)" -ne 0 ]; then
        if available sudo; then
            SUDO=sudo
        elif [ -n "$DRY_RUN" ]; then
            warning "no sudo here; the install would need root"
        else
            error "this script needs root to write under /usr/local: run it as root, or install sudo"
        fi
    fi
    require curl tar sha256sum awk grep

    BINDIR=
    for dir in /usr/local/bin /usr/bin /bin; do
        case ":$PATH:" in
            *":$dir:"*) BINDIR=$dir; break ;;
        esac
    done
    [ -n "$BINDIR" ] || error "none of /usr/local/bin, /usr/bin, /bin is on PATH"
    INSTALL_DIR=$(dirname "$BINDIR")

    obtain
    stop_app
    status "Installing under $INSTALL_DIR: bin/boxdesk, bin/$APP, share/applications, share/icons, share/boxdesk"
    as_root rm -rf "$INSTALL_DIR/share/boxdesk"
    # root's tar takes ownership and mode from the archive by default, so an archive naming a
    # setuid file would be obeyed. The release carries none; these say so rather than trust it.
    as_root tar --no-same-owner --no-same-permissions -xzf "$TEMP_DIR/$ASSET" -C "$INSTALL_DIR"
    as_root chmod 0755 "$BINDIR/boxdesk" "$BINDIR/$APP"
    if available update-desktop-database; then
        as_root update-desktop-database "$INSTALL_DIR/share/applications" 2>/dev/null || true
    fi

    install_rootfs "$INSTALL_DIR/share/boxdesk/rootfs.tar.gz"
    ensure_libkrun_linux

    if ! kvm_usable; then
        who="${SUDO_USER:-$(id -un)}"
        warning "/dev/kvm is not readable and writable by $who: a sandbox will not boot."
        warning "That usually means membership of the kvm group: sudo usermod -aG kvm $who, then a new login."
    fi
    status "Install complete. Run 'boxdesk' from the command line, or start $APP from the desktop's launcher."
}

cleanup() { rm -rf "$TEMP_DIR"; }

main() {
    for arg in "$@"; do
        case "$arg" in
            --dry-run) DRY_RUN=1 ;;
            --yes | -y) YES=1 ;;
            *) error "unknown argument $arg (this script takes --dry-run and --yes)" ;;
        esac
    done
    [ -z "$DRY_RUN" ] || status "Dry run: each command that would change this machine is printed after '+', and none runs"

    OS=$(uname -s)
    ARCH=$(uname -m)
    case "$OS/$ARCH" in
        Darwin/arm64) ASSET="Boxdesk-macos-aarch64.zip" ;;
        Linux/x86_64) ASSET="boxdesk-linux-x86_64.tgz" ;;
        *) error "boxdesk has a release for macOS on ARM64 (Apple silicon) and for Linux on x86_64; this is $OS on $ARCH" ;;
    esac

    VERSION="${BOXDESK_VERSION:-}"
    if [ -n "$VERSION" ]; then
        BASE_URL="https://github.com/$REPO/releases/download/v${VERSION#v}"
    else
        BASE_URL="https://github.com/$REPO/releases/latest/download"
    fi

    TEMP_DIR=$(mktemp -d)
    trap cleanup EXIT

    case "$OS" in
        Darwin) install_macos ;;
        Linux) install_linux ;;
    esac

    if [ -n "$MISSING_LIBKRUN" ]; then
        warning "The binaries are installed, but libkrun is not, so no sandbox boots yet. Run the line above, then 'boxdesk run -- uname -a'."
        exit 1
    fi
}

main "$@"
