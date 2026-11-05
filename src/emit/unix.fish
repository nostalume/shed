# Captures are NUL-framed (value, status) pairs, consumed by string split0.
# No newline trimming/splitting and no named scratch variables are involved.
function __shed_capture
    if $argv
        builtin printf '\000%s\000' 0
    else
        switch $status
            case 1
                builtin printf '\000%s\000' 1
            case '*'
                builtin printf '\000%s\000' 2
        end
    end
end
function __shed_system
    if builtin test -x /usr/bin/$argv[1]
        command /usr/bin/$argv[1] $argv[2..-1]
    else
        command /bin/$argv[1] $argv[2..-1]
    end
end
function __shed_env
    # argv is Fish's intrinsic argument slot, not a normal environment binding.
    builtin test "$argv[1]" != argv; or builtin return 2
    builtin set argv (__shed_capture __shed_system printenv "$argv[1]" 2>/dev/null | builtin string split0)
    switch "$argv[2]"
        case 0
            builtin printf '%s' (builtin string split -r -m1 -f1 \n -- "$argv[1]") | __shed_system iconv -f UTF-8 -t UTF-8 2>/dev/null
            or builtin return 2
        case 1
            builtin return 1
        case '*'
            builtin return 2
    end
end
function __shed_home
    builtin string match -q -r '\x00' -- "$HOME"; and builtin return 2
    if not builtin string match -q '/*' -- "$HOME"; or not builtin test -d "$HOME"
        builtin return 2
    end
    builtin printf '%s' "$HOME" | __shed_system iconv -f UTF-8 -t UTF-8 2>/dev/null
    or builtin return 2
end
function __shed_word
    while builtin test (builtin count $argv) -gt 0
        switch "$argv[1]"
            case l
                builtin printf '%s' "$argv[2]"; or builtin return 2
            case e
                if __shed_env "$argv[2]"
                    builtin true
                else
                    builtin test $status -eq 1; or builtin return 2
                end
            case h
                __shed_home; or builtin return 2
        end
        builtin set argv $argv[3..-1]
    end
end
function __shed_pwd
    builtin pwd -P
end
function __shed_absolute
    if builtin string match -q '/*' -- "$argv[1]"
        builtin printf '%s' "$argv[1]"
        builtin return
    end
    builtin set argv "$argv[1]" (__shed_capture __shed_pwd | builtin string split0)
    builtin test "$argv[3]" -eq 0; or builtin return 2
    builtin set argv "$argv[1]" (builtin string split -r -m1 -f1 \n -- "$argv[2]")
    builtin test -n "$argv[2]"; or builtin return 2
    if builtin test "$argv[2]" = /
        builtin printf '/%s' "$argv[1]"
    else
        builtin printf '%s/%s' "$argv[2]" "$argv[1]"
    end | __shed_system iconv -f UTF-8 -t UTF-8 2>/dev/null
    or builtin return 2
end
function __shed_exists
    builtin test -n "$argv[1]"; or builtin return 1
    builtin set argv (__shed_capture __shed_absolute "$argv[1]" | builtin string split0)
    builtin test "$argv[2]" -eq 0; or builtin return 2
    builtin test -e "$argv[1]"
end
function __shed_env_truth
    builtin set argv (__shed_capture __shed_env "$argv[1]" | builtin string split0)
    switch "$argv[2]"
        case 0
            builtin test -n "$argv[1]"
        case 1
            builtin return 1
        case '*'
            builtin return 2
    end
end
function __shed_resolve
    builtin test -n "$argv[1]"; or builtin return 1
    if builtin string match -q '*/*' -- "$argv[1]"
        builtin set argv (__shed_capture __shed_absolute "$argv[1]" | builtin string split0)
        builtin test "$argv[2]" -eq 0; or builtin return 2
        if builtin test -f "$argv[1]"; and builtin test -x "$argv[1]"
            builtin printf '%s' "$argv[1]"
        else
            builtin return 1
        end
        builtin return
    end
    builtin set argv "$argv[1]" (__shed_capture __shed_env PATH | builtin string split0)
    switch "$argv[3]"
        case 1
            builtin return 1
        case 0
        case '*'
            builtin return 2
    end
    builtin set argv "$argv[1]" (builtin string split ':' -- "$argv[2]")
    while builtin test (builtin count $argv) -gt 1
        if builtin test -z "$argv[2]"
            builtin set argv "$argv[1]" . $argv[3..-1]
        end
        builtin set argv "$argv[1]" (__shed_capture __shed_absolute "$argv[2]/$argv[1]" | builtin string split0) $argv[3..-1]
        builtin test "$argv[3]" -eq 0; or builtin return 2
        if builtin test -f "$argv[2]"; and builtin test -x "$argv[2]"
            builtin printf '%s' "$argv[2]"
            builtin return
        end
        builtin set argv "$argv[1]" $argv[4..-1]
    end
    builtin return 1
end
function __shed_have
    builtin set argv (__shed_capture __shed_resolve "$argv[1]" | builtin string split0)
    builtin return $argv[2]
end
function __shed_set
    builtin test "$argv[1]" != argv; or builtin return 2
    if builtin test "$argv[1]" = PATH
        # Fish rewrites empty PATH entries to '.'. Reject before native mutation.
        if builtin string match -q -r '(^$|^:|:$|::)' -- "$argv[2]"
            builtin printf '%s\n' 'shed: Fish cannot preserve empty PATH entries' >&2
            builtin return 2
        end
        # No fish_add_path: nonexistent entries and exact old spellings survive.
        builtin true
        builtin set -gx --path PATH (builtin string split ':' -- "$argv[2]") 2>/dev/null
    else
        # Assignment-mode set preserves the preceding status, including this
        # if's false test. Establish success before checking assignment failure.
        builtin true
        builtin set -gx --unpath -- "$argv[1]" "$argv[2]"; or builtin return 2
    end
    builtin set argv "$argv[2]" (__shed_capture __shed_env "$argv[1]" | builtin string split0)
    builtin test "$argv[3]" -eq 0; and builtin test "$argv[1]" = "$argv[2]"
    or builtin return 2
end
function __shed_unset
    builtin test "$argv[1]" != argv; or builtin return 2
    if __shed_system printenv "$argv[1]" >/dev/null 2>&1
        builtin set -e -- "$argv[1]"; or builtin return 2
    else
        builtin test $status -eq 1; or builtin return 2
        builtin return 0
    end
    if __shed_system printenv "$argv[1]" >/dev/null 2>&1
        builtin return 2
    else
        builtin test $status -eq 1
    end
end
function __shed_filter_path
    builtin set argv "$argv[1]" (builtin string split ':' -- "$argv[2]")
    while builtin test (builtin count $argv) -gt 1
        if builtin test "$argv[1]" != "$argv[2]"
            builtin printf ':%s' "$argv[2]"; or builtin return 2
        end
        builtin set argv "$argv[1]" $argv[3..-1]
    end
end
function __shed_path
    if builtin test -z "$argv[1]"; or builtin string match -q '*:*' -- "$argv[1]"
        builtin return 2
    end
    builtin set argv (__shed_capture __shed_absolute "$argv[1]" | builtin string split0) "$argv[2]"
    builtin test "$argv[2]" -eq 0; or builtin return 2
    builtin set argv "$argv[1]" "$argv[3]" (__shed_capture __shed_env PATH | builtin string split0)
    builtin string match -q '*:*' -- "$argv[1]"; and builtin return 2
    switch "$argv[4]"
        case 1
            __shed_set PATH "$argv[1]"
            builtin return
        case 0
            builtin set argv "$argv[1]" "$argv[2]" (__shed_capture __shed_filter_path "$argv[1]" "$argv[3]" | builtin string split0)
        case '*'
            builtin return 2
    end
    builtin test "$argv[4]" -eq 0; or builtin return 2
    if builtin test "$argv[2]" = prepend
        __shed_set PATH "$argv[1]$argv[3]"
    else if builtin test -n "$argv[3]"
        __shed_set PATH (builtin string split -m1 -f2 ':' -- "$argv[3]")':'"$argv[1]"
    else
        __shed_set PATH "$argv[1]"
    end
end
function __shed_run
    builtin string match -q -r '\x00' -- $argv; and builtin return 2
    builtin printf '%s\000' $argv | __shed_system iconv -f UTF-8 -t UTF-8 >/dev/null 2>&1
    or builtin return 2
    builtin set argv (__shed_capture __shed_resolve "$argv[1]" | builtin string split0) $argv[2..-1]
    builtin test "$argv[2]" -eq 0; or begin
        builtin printf '%s\n' 'shed: external executable unavailable' >&2
        builtin return 127
    end
    command "$argv[1]" $argv[3..-1]
end
function __shed_valid_file
    __shed_system iconv -f UTF-8 -t UTF-8 "$argv[1]" >/dev/null 2>&1; or builtin return 2
    __shed_system tr -d '\000' < "$argv[1]" | __shed_system cmp -s "$argv[1]" -
    __shed_pipeline_ok $pipestatus
end
function __shed_pipeline_ok
    not builtin string match -q -v 0 -- $argv
end
function __shed_temp
    if builtin set -q TMPDIR; and builtin test -n "$TMPDIR"
        __shed_system mktemp "$TMPDIR/shed.XXXXXXXXXX"
    else
        __shed_system mktemp '/tmp/shed.XXXXXXXXXX'
    end
end
function __shed_init
    builtin set argv (__shed_capture __shed_temp | builtin string split0) $argv
    builtin test "$argv[2]" -eq 0; or begin
        builtin printf 'return 1\n'
        builtin return 1
    end
    builtin set argv (builtin string split -r -m1 -f1 \n -- "$argv[1]") $argv[3..-1]
    if __shed_run $argv[2..-1] > "$argv[1]"; and __shed_valid_file "$argv[1]"
        begin
            if __shed_system rm -f -- "$argv[1]"
                __shed_system cat; or builtin printf '\nreturn 1\n'
            else
                builtin printf 'return 1\n'
            end
        end < "$argv[1]"
    else
        __shed_system rm -f -- "$argv[1]"
        builtin printf '%s\n' 'shed: initializer failed or emitted invalid text' >&2
        builtin printf 'return 1\n'
    end
end
