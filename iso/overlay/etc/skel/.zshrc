# Enable Powerlevel10k instant prompt. Should stay close to the top of ~/.zshrc.
# Skip on HDMI/serial: the Linux VT cannot render Nerd Fonts/powerline, and
# instant prompt forks extra zsh processes that freeze the console.
_pertisk_tty="$(tty 2>/dev/null || true)"
_pertisk_plain=
[[ -n "${PERTISK_CONSOLE:-}" ]] && _pertisk_plain=1
case "${TERM:-}" in
  linux|dumb|vt220|vt100|vt102|ansi) _pertisk_plain=1 ;;
esac
case "${_pertisk_tty}" in
  /dev/tty[0-9]*) _pertisk_plain=1 ;;
  /dev/ttyS*|/dev/ttyAMA*|/dev/ttyUSB*|/dev/ttyAML*) _pertisk_plain=1 ;;
esac

if [[ -z "${_pertisk_plain:-}" && -r "${XDG_CACHE_HOME:-$HOME/.cache}/p10k-instant-prompt-${(%):-%n}.zsh" ]]; then
  source "${XDG_CACHE_HOME:-$HOME/.cache}/p10k-instant-prompt-${(%):-%n}.zsh"
fi

# pertisk-vm host shell: Oh My Zsh + Powerlevel10k
# Prompt style is the shipped ~/.p10k.zsh (from `p10k configure`). Re-run that
# only to change the look; image/deploy copies this file so the wizard is reused.

export PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin${PATH:+:$PATH}"

if [[ -n "${_pertisk_plain:-}" ]]; then
  unset _pertisk_plain _pertisk_tty
  autoload -Uz colors && colors
  PROMPT='%F{green}%n@%m%f %F{cyan}%~%f %# '
  RPROMPT=
  return
fi
unset _pertisk_plain _pertisk_tty

export ZSH="${ZSH:-/usr/share/oh-my-zsh}"
ZSH_THEME="powerlevel10k/powerlevel10k"
plugins=(git)

DISABLE_AUTO_UPDATE="true"
DISABLE_UPDATE_PROMPT="true"
ZSH_DISABLE_COMPFIX="true"
# Do not auto-start the wizard; the overlay already has a configured ~/.p10k.zsh.
POWERLEVEL9K_DISABLE_CONFIGURATION_WIZARD=true

if [[ -f "$ZSH/oh-my-zsh.sh" ]]; then
  source "$ZSH/oh-my-zsh.sh"
else
  autoload -Uz colors && colors
  PROMPT='%F{cyan}%n@%m%f %F{yellow}%~%f %# '
fi

[[ -r /usr/share/zsh-autosuggestions/zsh-autosuggestions.zsh ]] \
  && source /usr/share/zsh-autosuggestions/zsh-autosuggestions.zsh
[[ -r /usr/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh ]] \
  && source /usr/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh

# To customize prompt, run `p10k configure` or edit ~/.p10k.zsh.
[[ ! -f ~/.p10k.zsh ]] || source ~/.p10k.zsh
