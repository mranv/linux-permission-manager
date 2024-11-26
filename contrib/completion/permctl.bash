#!/usr/bin/env bash

_permctl() {
    local i cur prev opts cmds
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"
    cmd=""
    opts=""

    for i in ${COMP_WORDS[@]}
    do
        case "${i}" in
            permctl)
                cmd="permctl"
                ;;
            
            cleanup)
                cmd+="__cleanup"
                ;;
            commands)
                cmd+="__commands"
                ;;
            grant)
                cmd+="__grant"
                ;;
            help)
                cmd+="__help"
                ;;
            init)
                cmd+="__init"
                ;;
            list)
                cmd+="__list"
                ;;
            revoke)
                cmd+="__revoke"
                ;;
            verify)
                cmd+="__verify"
                ;;
            *)
                ;;
        esac
    done

    case "${cmd}" in
        permctl)
            opts="grant revoke list commands cleanup init verify help"
            COMPREPLY=( $(compgen -W "${opts}" -- ${cur}) )
            return 0
            ;;
        
        permctl__grant)
            case "${prev}" in
                grant)
                    COMPREPLY=( $(compgen -u -- ${cur}) )
                    return 0
                    ;;
                -d|--duration)
                    COMPREPLY=( $(compgen -W "30 60 120 240 480" -- ${cur}) )
                    return 0
                    ;;
                *)
                    if [[ ${COMP_CWORD} -eq 3 ]]; then
                        # Complete with allowed commands from config
                        if [ -f /etc/permctl/config.yaml ]; then
                            COMPREPLY=( $(compgen -W "$(permctl commands | grep -v Allowed | tr -d ' ')" -- ${cur}) )
                        fi
                    else
                        opts="-d --duration"
                        COMPREPLY=( $(compgen -W "${opts}" -- ${cur}) )
                    fi
                    return 0
                    ;;
            esac
            ;;
            
        permctl__revoke)
            case "${prev}" in
                revoke)
                    COMPREPLY=( $(compgen -u -- ${cur}) )
                    return 0
                    ;;
                *)
                    if [[ ${COMP_CWORD} -eq 3 ]]; then
                        # Complete with allowed commands from config
                        if [ -f /etc/permctl/config.yaml ]; then
                            COMPREPLY=( $(compgen -W "$(permctl commands | grep -v Allowed | tr -d ' ')" -- ${cur}) )
                        fi
                    fi
                    return 0
                    ;;
            esac
            ;;
            
        permctl__list)
            opts="-a --all -u --user"
            COMPREPLY=( $(compgen -W "${opts}" -- ${cur}) )
            return 0
            ;;
            
        permctl__commands)
            opts="-v --verbose"
            COMPREPLY=( $(compgen -W "${opts}" -- ${cur}) )
            return 0
            ;;
            
        permctl__init)
            opts="-f --force"
            COMPREPLY=( $(compgen -W "${opts}" -- ${cur}) )
            return 0
            ;;
    esac
}

complete -F _permctl permctl