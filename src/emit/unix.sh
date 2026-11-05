# Bash/Zsh scratch state uses positional parameters, never environment names.
# Query statuses: 0=true, 1=false, 2=error. Capture suffixes retain trailing LF.
__shed_capture() {
    if "$@"; then \builtin printf 0
    else case $? in 1) \builtin printf 1;; *) \builtin printf 2;; esac
    fi
}
__shed_system() { \builtin command -p "$@"; }
__shed_env() {
    \builtin set -- "$(__shed_capture __shed_system printenv "$1" 2>/dev/null)"
    case "$1" in
        *0) \builtin set -- "${1%?}"
            \builtin printf '%s' "${1%?}" | __shed_system iconv -f UTF-8 -t UTF-8 2>/dev/null || \builtin return 2;;
        *1) \builtin return 1;; *) \builtin return 2;;
    esac
}
__shed_home() {
    \builtin set -- "${HOME-}"
    __shed_nul_free "$1" || \builtin return 2
    case "$1" in /*) ;; *) \builtin return 2;; esac
    \builtin test -d "$1" || \builtin return 2
    \builtin printf '%s' "$1" | __shed_system iconv -f UTF-8 -t UTF-8 2>/dev/null || \builtin return 2
}
__shed_word() {
    while \builtin test "$#" -ne 0; do
        case "$1" in
            l) \builtin printf '%s' "$2" || \builtin return 2;;
            e) __shed_env "$2" || { \builtin test "$?" -eq 1 || \builtin return 2; };;
            h) __shed_home || \builtin return 2;;
        esac
        \builtin shift 2
    done
}
__shed_absolute() {
    case "$1" in /*) \builtin printf '%s' "$1";; *)
        \builtin set -- "$1" "$(__shed_capture \builtin pwd -P)"
        case "$2" in *0) ;; *) \builtin return 2;; esac
        \builtin set -- "$1" "${2%?}"
        \builtin set -- "$1" "${2%?}"
        \builtin test -n "$2" || \builtin return 2
        \builtin printf '%s/%s' "${2%/}" "$1" | __shed_system iconv -f UTF-8 -t UTF-8 2>/dev/null || \builtin return 2;;
    esac
}
__shed_exists() {
    \builtin test -n "$1" || \builtin return 1
    \builtin set -- "$(__shed_capture __shed_absolute "$1")"
    case "$1" in *0) \builtin test -e "${1%?}";; *) \builtin return 2;; esac
}
__shed_env_truth() {
    \builtin set -- "$(__shed_capture __shed_env "$1")"
    case "$1" in *0) \builtin test -n "${1%?}";; *1) \builtin return 1;; *) \builtin return 2;; esac
}
__shed_resolve() {
    \builtin test -n "$1" || \builtin return 1
    case "$1" in */*)
        \builtin set -- "$(__shed_capture __shed_absolute "$1")"
        case "$1" in *0) \builtin set -- "${1%?}";; *) \builtin return 2;; esac
        if \builtin test -f "$1" && \builtin test -x "$1"; then \builtin printf '%s' "$1"; else \builtin return 1; fi
        \builtin return;;
    esac
    \builtin set -- "$1" "$(__shed_capture __shed_env PATH)"
    case "$2" in *0) \builtin set -- "$1" "${2%?}";; *1) \builtin return 1;; *) \builtin return 2;; esac
    while :; do
        case "$2" in
            *:*) \builtin set -- "$1" "${2#*:}" "${2%%:*}" 1;;
            *) \builtin set -- "$1" '' "$2" 0;;
        esac
        \builtin set -- "$1" "$2" "$(__shed_capture __shed_absolute "${3:-.}/$1")" "$4"
        case "$3" in *0) \builtin set -- "$1" "$2" "${3%?}" "$4";; *) \builtin return 2;; esac
        if \builtin test -f "$3" && \builtin test -x "$3"; then __shed_absolute "$3"; \builtin return; fi
        \builtin test "$4" -eq 1 || \builtin return 1
    done
}
__shed_have() {
    \builtin set -- "$(__shed_capture __shed_resolve "$1")"
    case "$1" in *0) \builtin return 0;; *1) \builtin return 1;; *) \builtin return 2;; esac
}
__shed_set() {
    # Integer/array/nameref slots can interpret data as native expressions.
    __shed_assignable "$1" || \builtin return 2
    \builtin export "$1=$2" || \builtin return 2
    \builtin set -- "$1" "$2" "$(__shed_capture __shed_env "$1")"
    \builtin test "$3" = "${2}0" || \builtin return 2
}
__shed_unset() {
    if __shed_system printenv "$1" >/dev/null 2>&1; then
        \builtin unset -v "$1" || \builtin return 2
    else
        \builtin test "$?" -eq 1 || \builtin return 2
        \builtin return 0
    fi
    if __shed_system printenv "$1" >/dev/null 2>&1; then \builtin return 2
    else \builtin test "$?" -eq 1 || \builtin return 2
    fi
}
__shed_filter_path() {
    # A leading colon per retained entry distinguishes [] from [""].
    while :; do
        case "$2" in
            *:*) \builtin set -- "$1" "${2#*:}" "${2%%:*}" 1;;
            *) \builtin set -- "$1" '' "$2" 0;;
        esac
        if \builtin test "$3" != "$1"; then \builtin printf ':%s' "$3" || \builtin return 2; fi
        \builtin test "$4" -eq 1 || \builtin return 0
    done
}
__shed_path() {
    case "$1" in ''|*:*) \builtin return 2;; esac
    \builtin set -- "$(__shed_capture __shed_absolute "$1")" "$2"
    case "$1" in *0) \builtin set -- "${1%?}" "$2";; *) \builtin return 2;; esac
    case "$1" in *:*) \builtin return 2;; esac
    \builtin set -- "$1" "$2" "$(__shed_capture __shed_env PATH)"
    case "$3" in
        *1) __shed_set PATH "$1"; \builtin return;;
        *0) \builtin set -- "$1" "$2" "$(__shed_capture __shed_filter_path "$1" "${3%?}")";;
        *) \builtin return 2;;
    esac
    case "$3" in *0) \builtin set -- "$1" "$2" "${3%?}";; *) \builtin return 2;; esac
    if \builtin test "$2" = prepend; then __shed_set PATH "$1$3"
    elif \builtin test -n "$3"; then __shed_set PATH "${3#:}:$1"
    else __shed_set PATH "$1"
    fi
}
__shed_run() {
    __shed_nul_free "$@" || \builtin return 2
    \builtin printf '%s\000' "$@" | __shed_system iconv -f UTF-8 -t UTF-8 >/dev/null 2>&1 || \builtin return 2
    \builtin set -- "$(__shed_capture __shed_resolve "$1")" "${@:2}"
    case "$1" in
        *0) \builtin command "${1%?}" "${@:2}";;
        *) \builtin printf '%s\n' 'shed: external executable unavailable' >&2; \builtin return 127;;
    esac
}
__shed_valid_file() (
    \builtin set -o pipefail
    __shed_system iconv -f UTF-8 -t UTF-8 "$1" >/dev/null 2>&1 &&
        { __shed_system tr -d '\000' < "$1" | __shed_system cmp -s "$1" -; }
)
__shed_init() (
    # This subshell owns its capture file; both completion paths remove it.
    \builtin set -- "$(__shed_capture __shed_system mktemp "${TMPDIR:-/tmp}/shed.XXXXXXXXXX")" "$@"
    case "$1" in *0) ;; *) \builtin printf 'return 1\n'; \builtin return 1;; esac
    \builtin set -- "${1%?}" "${@:2}"
    \builtin set -- "${1%?}" "${@:2}"
    if __shed_run "${@:2}" > "$1" && __shed_valid_file "$1"; then
        __shed_system cat "$1" || \builtin printf '\nreturn 1\n'
    else
        \builtin printf '%s\n' 'shed: initializer failed or emitted invalid text' >&2
        \builtin printf 'return 1\n'
    fi
    __shed_system rm -f -- "$1" || \builtin printf '\nreturn 1\n'
)
