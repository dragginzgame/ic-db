#!/usr/bin/env perl
# Exercise the real adapter/logger and Make recipes; expensive leaves are substitutes.
use strict;
use warnings;
use Test::More;
use File::Temp qw(tempdir);
use File::Path qw(make_path);
use File::Copy qw(copy);
use Cwd qw(getcwd);
use JSON::PP;

my $root = getcwd();
my $temp = tempdir(CLEANUP => 1);
make_path("$temp/scripts", "$temp/vendor", "$temp/.tools/host/bin");
system('cp', '-R', "$root/vendor/toko-tooling", "$temp/vendor/") == 0 or die 'copy tooling';
copy('scripts/release.sh', "$temp/scripts/release.sh") or die $!;
copy('Makefile', "$temp/Consumer.mk") or die $!;

sub write_file {
    my ($path, $bytes) = @_;
    open my $f, '>', $path or die "$path: $!";
    print {$f} $bytes;
    close $f or die $!;
}
sub read_file {
    open my $f, '<', $_[0] or die "$_[0]: $!";
    local $/; return <$f> // '';
}
sub run {
    my (@command) = @_;
    my $pid = open my $out, '-|';
    defined $pid or die $!;
    if (!$pid) {
        chdir $temp or die $!;
        delete @ENV{qw(MAKEFLAGS MFLAGS MAKELEVEL MAKEFILES GITHUB_ACTIONS GITHUB_STEP_SUMMARY
            VALIDATION_INPUT_BASELINE VALIDATION_INPUT_ROOT VALIDATION_RUNNER_DEPTH)};
        $ENV{GITHUB_STEP_SUMMARY} = $ENV{TEST_SUMMARY} if $ENV{TEST_SUMMARY};
        open STDERR, '>&', STDOUT or die $!;
        exec @command; die $!;
    }
    local $/; my $printed = <$out> // '';
    close $out;
    return ($? >> 8, $printed);
}

my @gates = qw(release-setup tools-check release-lint fmt-check test doc docs-check m0-contracts
    m0-core m0-aggregation m0-maintenance m1-records-wasm m1-commit-failures
    m3-maintenance-wasm m3-cli);
my $leaf_makefile = '.PHONY: ' . join(' ', @gates) . "\n" .
    join('', map { "$_:\n\t\@echo $_ >> target/dispatch\n" .
        "\t\@if [ \"\$\$FAIL_GATE\" = $_ ]; then echo 'error: selected leaf failed'; exit 33; fi\n" }
        @gates);
write_file("$temp/Makefile", $leaf_makefile);
write_file("$temp/.gitignore", "/target/\n/.tools/\n/qualification/target/\n");
# Stop at the metadata owner: this test must not render/release packages.
write_file("$temp/scripts/release-metadata.sh",
    "select_metadata_editor() { echo reached-metadata >> target/dispatch; return 71; }\n");
for my $command (
    ['git', 'init', '-q'], ['git', 'config', 'user.name', 'Release fixture'],
    ['git', 'config', 'user.email', 'fixture@example.invalid'],
    ['git', 'add', '.'], ['git', '-c', 'core.hooksPath=/dev/null', 'commit', '-qm', 'fixture']
) {
    my ($status, $out) = run(@$command);
    is($status, 0, "fixture command @$command") or diag $out;
}
my (undef, $source) = run('git', 'rev-parse', 'HEAD'); chomp $source;
local $ENV{RELEASE_SOURCE} = $source;
local $ENV{PATH} = "$temp/.tools/host/bin:$ENV{PATH}";
make_path("$temp/target", "$temp/.git/release-state/ic-db");
my $receipt = "$temp/.git/release-state/ic-db/$source.verified";
write_file($receipt, "$source\n");
local $ENV{FAIL_GATE} = 'm0-core';
my ($status, $out) = run('bash', 'scripts/release.sh', 'release-verify');
isnt($status, 0, 'failed gate refuses validation');
ok(!-e $receipt, 'failed recheck invalidates an earlier whole-gate receipt');
is_deeply([split /\n/, read_file("$temp/target/dispatch")], [@gates[0..8]],
    'required failure stops later leaves');
my @attempts = glob "$temp/target/release-validation/$source/validation.*";
is(scalar @attempts, 2, 'setup and offline validation have separate retained logger directories');
my ($failed) = grep { read_file("$_/timings.tsv") =~ /m0-core\tFAIL\t/ } @attempts;
ok(defined $failed, 'failed phase and elapsed time retained');
like(read_file("$failed/7.log"), qr/selected leaf failed/, 'original failure output retained');
my $failed_timing = read_file("$failed/timings.tsv");

write_file("$temp/target/dispatch", '');
$ENV{FAIL_GATE} = '';
($status, $out) = run('bash', 'scripts/release.sh', 'release-verify');
is($status, 0, 'retry completes actual adapter and shared logger') or diag $out;
is_deeply([split /\n/, read_file("$temp/target/dispatch")], \@gates,
    'retry executes the complete roster once; partial logs never skip checks');
is(read_file($receipt), "$source\n", 'only complete success writes source receipt');
is(read_file("$failed/timings.tsv"), $failed_timing, 'retry preserves original failure evidence');
@attempts = glob "$temp/target/release-validation/$source/validation.*";
is(scalar @attempts, 4, 'retry retains separate setup/validation timing evidence');

# A post-execution evidence failure must not admit the gate or discard leaf logs.
local $ENV{TEST_SUMMARY} = "$temp/missing-directory/summary";
write_file("$temp/target/dispatch", '');
($status, $out) = run('bash', 'scripts/release.sh', 'release-verify');
isnt($status, 0, 'failed evidence handoff refuses validation');
ok(!-e $receipt, 'evidence handoff failure cannot leave a valid receipt');
is(read_file("$temp/target/dispatch"), "release-setup\n", 'failed setup evidence withholds offline gate');
like($out, qr/complete summary retained/, 'handoff failure reports preserved evidence');
$ENV{TEST_SUMMARY} = '';
($status, $out) = run('bash', 'scripts/release.sh', 'release-verify');
is($status, 0, 'repaired evidence handoff permits complete retry');
($status, $out) = run('bash', 'scripts/release.sh', 'release-prepare-version');
is($status, 71, 'successful whole gate reaches metadata owner, without rerunning validation');
for my $case (qw(missing tampered symlink)) {
    unlink $receipt or die $!;
    write_file($receipt, ('0' x 40) . "\n") if $case eq 'tampered';
    symlink "$temp/target/dispatch", $receipt or die $! if $case eq 'symlink';
    my $before = read_file("$temp/target/dispatch");
    ($status, $out) = run('bash', 'scripts/release.sh', 'release-prepare-version');
    isnt($status, 0, "$case receipt refuses preparation");
    is(read_file("$temp/target/dispatch"), $before, "$case receipt cannot reach metadata owner");
    unlink $receipt if -e $receipt || -l $receipt;
    write_file($receipt, "$source\n");
}
write_file("$temp/Makefile", read_file("$temp/Makefile") . "\n# source drift\n");
my $before = read_file("$temp/target/dispatch");
($status, $out) = run('bash', 'scripts/release.sh', 'release-verify');
isnt($status, 0, 'changed source refuses before dispatch');
is(read_file("$temp/target/dispatch"), $before, 'changed source runs no leaf');
ok(!-e $receipt, 'changed source cannot retain the earlier receipt');

# Dispatch the actual consumer Make recipes with recorded Cargo/server substitutes.
copy("$temp/Consumer.mk", "$temp/Makefile") or die $!;
write_file("$temp/scripts/with-pocketic.sh", "#!/usr/bin/env bash\nexec \"\$\@\"\n");
write_file("$temp/.tools/host/bin/cargo", <<'CARGO');
#!/usr/bin/env perl
use strict; use warnings; use JSON::PP;
open my $log, '>>', 'target/cargo.jsonl' or die $!;
print {$log} encode_json(\@ARGV), "\n"; close $log;
if ($ENV{FAIL_BUILD} && $ARGV[0] eq 'build') { exit 42; }
if ($ARGV[0] eq 'build') {
    open my $wasm, '>', 'target/fixture.wasm' or die $!; print {$wasm} 'fixture'; close $wasm;
    print encode_json({reason=>'compiler-artifact', target=>{name=>'ic_db_m0_rollback_canister'},
        filenames=>['target/fixture.wasm']}), "\n";
}
CARGO
chmod 0755, "$temp/.tools/host/bin/cargo" or die $!;
($status, $out) = run('make', '--no-print-directory', '-f', 'Consumer.mk', 'm0-core');
is($status, 0, 'combined target dispatches actual build/test recipes') or diag $out;
my @commands = map { decode_json($_) } split /\n/, read_file("$temp/target/cargo.jsonl");
is(scalar @commands, 2, 'one build and one test invocation');
my @tests;
for my $i (0..$#{$commands[1]}-1) {
    push @tests, $commands[1][$i+1] if $commands[1][$i] eq '--test';
}
is_deeply(\@tests, [qw(entrypoints rollback batch reads)], 'all four original test binaries retained');
write_file("$temp/target/cargo.jsonl", '');
local $ENV{FAIL_BUILD} = 1;
($status, $out) = run('make', '--no-print-directory', '-f', 'Consumer.mk', 'm0-core');
isnt($status, 0, 'build failure blocks installed checks even with an old artifact');
is(scalar(split /\n/, read_file("$temp/target/cargo.jsonl")), 1, 'no test after failed build');
$ENV{FAIL_BUILD} = 0;
write_file("$temp/target/cargo.jsonl", '');
($status, $out) = run('make', '--no-print-directory', '-f', 'Consumer.mk', 'release-lint');
is($status, 0, 'release lint dispatches actual product and fixture selections') or diag $out;
@commands = map { decode_json($_) } split /\n/, read_file("$temp/target/cargo.jsonl");
ok(!grep({ join(' ', @$_) =~ /-p ic-db-icydb/ } @commands), 'no explicit frozen baseline lint');
my @wasm = grep { grep { $_ eq '--workspace' } @$_ } @commands;
is(scalar @wasm, 1, 'one workspace Wasm lint');
like(join(' ', @{$wasm[0]}), qr/--exclude ic-db-icydb-baseline-schema --exclude ic-db-icydb-baseline-canister/,
    'both frozen baseline packages excluded from routine release lint');

# Exercise the real shared phase runner through the local adapter. Only remote
# observations and expensive leaves are substitutes; never reach delivery.
unlink "$temp/.tools/host/bin/cargo" or die $!;
my (undef, $real_git) = run('sh', '-c', 'command -v git'); chomp $real_git;
local $ENV{REAL_GIT} = $real_git;
write_file("$temp/.tools/host/bin/git", <<'GIT');
#!/usr/bin/env perl
use strict; use warnings;
if ($ARGV[0] eq 'ls-remote') { exit 0; } # No candidate tag in the simulated remote.
die "fixture must never push\n" if $ARGV[0] eq 'push';
exec $ENV{REAL_GIT}, @ARGV; die $!;
GIT
write_file("$temp/.tools/host/bin/gh", "#!/bin/sh\nprintf '%s\\n' \"\${FIXTURE_VISIBILITY:-PUBLIC}\"\n");
chmod 0755, "$temp/.tools/host/bin/git", "$temp/.tools/host/bin/gh" or die $!;
write_file("$temp/Makefile", $leaf_makefile . <<'MAKE');
.PHONY: release-version release-preflight release-verify release-prepare-version
release-version:
	@printf '%s\n' 0.22.1
release-preflight release-verify release-prepare-version:
	@echo $@ >> target/hooks
	@bash scripts/release.sh $@
MAKE
write_file("$temp/CHANGELOG.md", "# Changelog\n\n## [0.22.2] - Unreleased\n\n### Changed\n\n- Fixture.\n\n## [0.22.1] - 2026-11-17\n\n### Changed\n\n- Previous.\n");
for my $command (
    ['git', 'branch', '-M', 'main'],
    ['git', 'remote', 'add', 'origin', 'https://github.com/dragginzgame/ic-db.git'],
    ['git', 'add', '.'], ['git', '-c', 'core.hooksPath=/dev/null', 'commit', '-qm', 'runner fixture']
) {
    my ($code, $printed) = run(@$command);
    is($code, 0, "runner fixture command @$command") or diag $printed;
}
my (undef, $runner_source) = run('git', 'rev-parse', 'HEAD'); chomp $runner_source;
$ENV{RELEASE_SOURCE} = $runner_source;
local $ENV{RELEASE_TIMING_JOURNAL} = "$temp/target/release-timing.jsonl";
local $ENV{RELEASE_TIMING_ATTEMPT} = 'failed-validation';
my @runner = ('bash', 'vendor/toko-tooling/scripts/ci/run-release.sh');
my $plan = "$temp/.git/release-state/0.22.2.plan";
write_file("$temp/target/dispatch", '');
$ENV{FAIL_GATE} = 'm0-core';
($status, $out) = run(@runner, 'patch', 'origin', 'main');
isnt($status, 0, 'actual runner stops on validation failure') or diag $out;
ok(!-e $plan, 'failed validation creates no post-validation recovery plan');
is_deeply([split /\n/, read_file("$temp/target/dispatch")], [@gates[0..8]],
    'runner preserves fail-fast local gate dispatch');

write_file("$temp/target/dispatch", '');
$ENV{FAIL_GATE} = '';
$ENV{RELEASE_TIMING_ATTEMPT} = 'complete-validation';
($status, $out) = run(@runner, 'patch', 'origin', 'main');
isnt($status, 0, 'metadata refusal stops actual runner');
is_deeply([split /\n/, read_file("$temp/target/dispatch")], [@gates, 'reached-metadata'],
    'full retry prepares and validates once before metadata');
like(read_file($plan), qr/\nprepare\n\z/, 'metadata failure retains prepare phase');
my $saved_plan = read_file($plan);
my $dispatch = read_file("$temp/target/dispatch");
my $runner_receipt = "$temp/.git/release-state/ic-db/$runner_source.verified";
write_file($runner_receipt, ('0' x 40) . "\n");
$ENV{RELEASE_TIMING_ATTEMPT} = 'tampered-receipt';
($status, $out) = run(@runner, 'resume', '0.22.2', 'origin', 'main');
isnt($status, 0, 'saved prepare phase refuses tampered source receipt');
is(read_file("$temp/target/dispatch"), $dispatch, 'tampered resume cannot reach metadata or rerun leaves');
is(read_file($plan), $saved_plan, 'refused resume preserves original recovery intent');
write_file($runner_receipt, "$runner_source\n");

local $ENV{FIXTURE_VISIBILITY} = 'PRIVATE';
$ENV{RELEASE_TIMING_ATTEMPT} = 'fresh-admission-refusal';
($status, $out) = run(@runner, 'resume', '0.22.2', 'origin', 'main');
isnt($status, 0, 'resume repeats fresh repository admission');
is(read_file("$temp/target/dispatch"), $dispatch, 'changed visibility withholds metadata and validation');
$ENV{FIXTURE_VISIBILITY} = 'PUBLIC';
$ENV{RELEASE_TIMING_ATTEMPT} = 'metadata-resume';
($status, $out) = run(@runner, 'resume', '0.22.2', 'origin', 'main');
isnt($status, 0, 'resumed attempt reaches deliberate metadata refusal');
is(read_file("$temp/target/dispatch"), $dispatch . "reached-metadata\n",
    'metadata recovery skips setup and validation but invokes its owner');
is(read_file($plan), $saved_plan, 'repeat metadata refusal preserves saved intent');
my (undef, $after_source) = run('git', 'rev-parse', 'HEAD'); chomp $after_source;
is($after_source, $runner_source, 'recovery fixture never creates a release commit');
my (undef, $tags) = run('git', 'tag', '--list');
is($tags, '', 'recovery fixture never tags');
($status, $out) = run('perl', 'vendor/toko-tooling/scripts/ci/release-timing.pl',
    'report', "$temp/target/release-timing.jsonl");
is($status, 0, 'shared phase timing evidence is readable');
my $timings = decode_json($out);
my @resumed = grep { $_->{attempt} eq 'metadata-resume' } @$timings;
is_deeply([map { $_->{phase} } @resumed], ['release-prepare'],
    'resumed timing has only the selected phase');
ok(!grep({ $_->{result} eq 'incomplete' } @$timings), 'all attempted phases retain completion or failure status');
for my $attempt (qw(failed-validation complete-validation metadata-resume)) {
    my @phases = grep { $_->{attempt} eq $attempt } @$timings;
    my $seconds = 0;
    $seconds += $_->{elapsed_seconds} for @phases;
    diag sprintf('%s: %.3f sec across %d phases (prepared tools, substituted leaves)',
        $attempt, $seconds, scalar @phases);
}
done_testing;
