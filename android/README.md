`debug.p12` is a throwaway debug signing key (store/key password `android`,
alias `androiddebugkey`). It is committed on purpose so every CI build is
signed with the same key and an installed APK can be updated in place. It
is not meant for releases.
