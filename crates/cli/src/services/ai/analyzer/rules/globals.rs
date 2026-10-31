//! Options each table program accepts before its subcommand: (name, takes a value).

pub(super) const NO_GLOBALS: &[(&str, bool)] = &[];

pub(super) const SYSTEMCTL_GLOBALS: &[(&str, bool)] = &[
    ("--user", false),
    ("--system", false),
    ("--no-pager", false),
    ("--no-legend", false),
    ("--no-ask-password", false),
    ("--no-block", false),
    ("--full", false),
    ("-l", false),
    ("--all", false),
    ("-a", false),
    ("--quiet", false),
    ("-q", false),
    ("--now", false),
    ("--force", false),
    ("-f", false),
    ("--failed", false),
    ("--show-types", false),
    ("-H", true),
    ("--host", true),
    ("-M", true),
    ("--machine", true),
    ("-n", true),
    ("--lines", true),
    ("-o", true),
    ("--output", true),
    ("-t", true),
    ("--type", true),
    ("--state", true),
    ("-p", true),
    ("--property", true),
    ("--root", true),
    ("--signal", true),
    ("-s", true),
    ("--kill-who", true),
];

pub(super) const DOCKER_GLOBALS: &[(&str, bool)] = &[
    ("-H", true),
    ("--host", true),
    ("--context", true),
    ("-c", true),
    ("--config", true),
    ("-l", true),
    ("--log-level", true),
    ("-D", false),
    ("--debug", false),
    ("--tls", false),
    ("--tlsverify", false),
    ("--tlscacert", true),
    ("--tlscert", true),
    ("--tlskey", true),
];

pub(super) const KUBECTL_GLOBALS: &[(&str, bool)] = &[
    ("-n", true),
    ("--namespace", true),
    ("--context", true),
    ("--kubeconfig", true),
    ("-s", true),
    ("--server", true),
    ("--user", true),
    ("--cluster", true),
    ("--as", true),
    ("--as-group", true),
    ("--request-timeout", true),
    ("-v", true),
    ("--v", true),
    ("--cache-dir", true),
    ("--insecure-skip-tls-verify", false),
    ("--certificate-authority", true),
    ("--tls-server-name", true),
];

pub(super) const HELM_GLOBALS: &[(&str, bool)] = &[
    ("-n", true),
    ("--namespace", true),
    ("--kube-context", true),
    ("--kubeconfig", true),
    ("--debug", false),
    ("--burst-limit", true),
];

pub(super) const GIT_GLOBALS: &[(&str, bool)] = &[
    ("-C", true),
    ("--no-pager", false),
    ("-P", false),
    ("--paginate", false),
    ("-p", false),
    ("--no-optional-locks", false),
    ("--git-dir", true),
    ("--work-tree", true),
    ("--bare", false),
    ("--no-replace-objects", false),
    ("--literal-pathspecs", false),
];

pub(super) const IP_GLOBALS: &[(&str, bool)] = &[
    ("-s", false),
    ("-d", false),
    ("-4", false),
    ("-6", false),
    ("-r", false),
    ("-o", false),
    ("-j", false),
    ("-p", false),
    ("-br", false),
    ("-c", false),
    ("-a", false),
    ("-t", false),
    ("-n", true),
    ("-netns", true),
];

pub(super) const APT_GLOBALS: &[(&str, bool)] = &[
    ("-y", false),
    ("--yes", false),
    ("-q", false),
    ("-qq", false),
    ("--quiet", false),
    ("-f", false),
    ("-s", false),
    ("--simulate", false),
    ("--dry-run", false),
    ("--no-install-recommends", false),
    ("--assume-yes", false),
    ("-V", false),
    ("-o", true),
    ("-t", true),
    ("--target-release", true),
];

pub(super) const SUPERVISOR_GLOBALS: &[(&str, bool)] = &[("-c", true), ("-s", true), ("-u", true)];

pub(super) const NPM_GLOBALS: &[(&str, bool)] = &[
    ("-g", false),
    ("--global", false),
    ("--json", false),
    ("-C", true),
    ("--prefix", true),
    ("--depth", true),
    ("--long", false),
    ("--silent", false),
];
