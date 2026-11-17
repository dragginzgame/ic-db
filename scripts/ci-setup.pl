#!/usr/bin/env perl
# Early CI setup: no installed tools, raw log upload, or environment dump required.
use strict;
use warnings;
use File::Path qw(make_path);
use File::Temp qw(tempdir);
use Digest::SHA qw(sha256_hex);
use Fcntl qw(O_WRONLY O_CREAT O_EXCL);

my $phase = shift // '';
$phase =~ /\A(?:compiler|tools|scanner)\z/ && @ARGV or die "usage: ci-setup.pl compiler|tools|scanner COMMAND [ARG ...]\n";
my $tool = {compiler => 'rustup', tools => 'make install-tools tools-check', scanner => 'gitleaks installer'}->{$phase};
my $root = $ENV{CI_SETUP_ROOT} // (($ENV{RUNNER_TEMP} // die "runner temp required\n") . '/ic-db-setup');
$root =~ m{\A/} && $root !~ /[\r\n]/ or die "absolute setup root required\n";
$ENV{CI_SETUP_ROOT} = $root;
$ENV{TOKO_TOOLING_STORE} = "$root/tools";
my @secrets = grep { length($_) >= 4 } map { $ENV{$_} }
    grep { /(?:TOKEN|SECRET|PASSWORD|CREDENTIAL|PRIVATE_KEY|API_KEY)/i } keys %ENV;

# Only complete bounded lines enter the retained tail. Discard oversized lines
# rather than clipping off a credential's label and exposing its remaining value.
sub excerpt {
    my ($input) = @_;
    my ($pending, $discard, $private, $omitted, $bytes) = ('', 0, 0, 0, 0);
    my @lines;
    my $keep = sub {
        my ($line) = @_;
        $line =~ s/\e\[[0-?]*[ -\/]*[\@-~]//g;
        $line =~ s/[^\x09\x20-\x7e]/?/g;
        if ($line =~ /-----BEGIN .*PRIVATE KEY-----/) { $private = 1; }
        my $redact = $private || $line =~ /(?:authorization|cookie|password|passwd|secret|token|credential|private[_ -]?key|api[_ -]?key)["']?\s*[=:]/i
            || $line =~ m{https?://[^\s/]+\@|https?://\S*\?}i
            || $line =~ /(?:gh[pousr]_|github_pat_|AKIA|ASIA)[A-Za-z0-9_]+|eyJ[A-Za-z0-9_-]+\./
            || $line =~ /[A-Za-z0-9+\/=]{80,}/;
        for my $secret (@secrets) { $redact ||= index($line, $secret) >= 0; }
        $private = 0 if $line =~ /-----END .*PRIVATE KEY-----/;
        $line = '[sensitive line omitted]' if $redact;
        push @lines, "$line\n";
        $bytes += length($lines[-1]);
        # Reserve space and one line for the explicit truncation marker.
        while ($bytes > 65472 || @lines > 299) { $bytes -= length(shift @lines); $omitted = 1; }
    };
    while (1) {
        my $read = sysread($input, my $chunk, 4096);
        defined $read or die "cannot read selected diagnostic\n";
        last if !$read;
        for my $part (split /(?<=\n)/, $chunk) {
            $pending .= $part unless $discard;
            $private = 1 if $pending =~ /-----BEGIN .*PRIVATE KEY-----/;
            if (length($pending) > 8192) { $pending = ''; $discard = 1; $omitted = 1; }
            if ($part =~ /\n\z/) {
                $pending =~ s/\r?\n\z//;
                $keep->($pending) unless $discard;
                ($pending, $discard) = ('', 0);
            }
        }
    }
    $keep->($pending) if length($pending) && !$discard;
    return ($omitted ? "[earlier output or oversized lines omitted]\n" : '')
        . (@lines ? join('', @lines) : "[no usable diagnostic output]\n");
}

my $pid = open my $output, '-|';
defined $pid or die "cannot launch setup\n";
if (!$pid) {
    open STDERR, '>&', STDOUT or die "cannot redirect setup stderr\n";
    exec { $ARGV[0] } @ARGV;
    die "cannot execute setup command\n";
}
my $log = eval { excerpt($output) };
my $read_failed = $@;
close $output;
my $wait = $?;
my $status = $wait & 127 ? 128 + ($wait & 127) : $wait >> 8;
if (!$status && !$read_failed) { print "$phase setup passed\n"; exit 0; }
$log //= "[setup output collection failed]\n";
print STDERR "$phase setup failed; original status=$status\n";
print STDERR "Showing the final bounded, sanitized excerpt; raw output is not retained.\n";
# Printed text is already sanitized, and prefixed to prevent workflow commands.
my @tail = split /\n/, $log;
my $shown = 0;
my @print;
while (@tail && $shown < 4096) { my $line = pop @tail; $shown += length($line); unshift @print, substr($line, 0, 4096); }
print STDERR "  $_\n" for @print;

my $collected = eval {
    -l $root and die "setup root is a symlink\n";
    make_path($root, {mode => 0700}) unless -d $root;
    my $evidence = tempdir("evidence-$phase.XXXXXX", DIR => $root);
    my $write = sub {
        my ($name, $content) = @_;
        sysopen my $file, "$evidence/$phase-$name.txt", O_WRONLY|O_CREAT|O_EXCL, 0600
            or die "cannot create diagnostic file\n";
        print {$file} $content or die "cannot write diagnostic file\n";
        close $file or die "cannot close diagnostic file\n";
    };
    $write->('output', $log);
    my $meta = "tool=$tool\nphase=$phase\noriginal_status=$status\n";
    for my $key (qw(GITHUB_RUN_ID GITHUB_RUN_ATTEMPT GITHUB_SHA)) {
        my $value = $ENV{$key} // 'local';
        $value =~ /\A[a-zA-Z0-9-]{1,64}\z/ or $value = 'omitted';
        $meta .= "$key=$value\n";
    }
    $meta .= "Only sanitized excerpts are retained. Raw files, environment, configs and binaries omitted.\n";
    # Explicit job-local roots only. Hidden directories are included, symlinks
    # never followed. Limit traversal, file count and reads independently.
    my @queue = map { ["$root/$_", $_, 0] } qw(tools assets);
    my ($visited, $logs) = (0, 0);
    while (@queue && $visited < 256 && $logs < 10) {
        my ($path, $relative, $depth) = @{shift @queue};
        $visited++;
        next if -l $path;
        if (-d $path && $depth < 4) {
            opendir my $dir, $path or die "cannot inspect attempt directory\n";
            my @entries;
            while (defined(my $name = readdir $dir)) {
                next if $name eq '.' || $name eq '..';
                push @entries, $name;
                last if @entries == 256;
            }
            closedir $dir;
            push @queue, map { ["$path/$_", "$relative/$_", $depth + 1] } sort @entries;
        } elsif (-f $path && $relative =~ m{/(?:install|download|stderr)\.log\z}) {
            # Omit giant logs whole; tailing raw bytes could lose redaction context.
            if (-s $path > 1024 * 1024) { $meta .= "oversized attempt log omitted\n"; next; }
            open my $file, '<', $path or die "cannot read attempt log\n";
            my $detail = excerpt($file);
            close $file or die "cannot close attempt log\n";
            $logs++;
            $write->("attempt-$logs", $detail);
            my ($name) = $relative =~ m{/([^/]+)\z};
            $meta .= "attempt-$logs=$name source_path_sha256=" . sha256_hex($relative) . "\n";
        }
    }
    $meta .= "No selected attempt logs present; setup may have failed before creating one.\n" if !$logs;
    $meta .= "Attempt selection capped; additional entries omitted.\n" if @queue;
    $write->('status', $meta);
    if (my $output = $ENV{GITHUB_OUTPUT}) {
        open my $file, '>>', $output or die "cannot publish evidence location\n";
        print {$file} "evidence=$evidence\n" or die "cannot publish evidence location\n";
        close $file or die "cannot publish evidence location\n";
    }
    print STDERR "Sanitized evidence: $evidence/$phase-*.txt (upload follows)\n";
    1;
};
print STDERR "Diagnostic collection failed; original setup status remains $status. Evidence may be missing or partial.\n" unless $collected;
exit($status || 1);
