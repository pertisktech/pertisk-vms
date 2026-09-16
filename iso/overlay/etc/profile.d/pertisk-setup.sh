# Force first-boot wizard on any interactive root login (safety net if getty
# does not run pertisk-console).
case "$-" in
  *i*)
    if [ "$(id -u 2>/dev/null)" = 0 ] \
      && [ -f /etc/pertisk/needs-setup ] \
      && [ ! -f /var/lib/pertisk/.setup-done ] \
      && [ -x /usr/sbin/pertisk-setup ] \
      && [ -z "${PERTISK_SETUP_RUNNING:-}" ]; then
      export PERTISK_SETUP_RUNNING=1
      exec /usr/sbin/pertisk-setup
    fi
    ;;
esac
