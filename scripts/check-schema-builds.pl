use strict;
use warnings;
use File::Find qw(find);
use JSON::PP qw(decode_json);

# These two fixtures exercise the complete producer/import-only consumer contract.
my %consumer_of = (
    'ic-db-m1-records-canister'     => 'ic-db-m1-records-schema',
    'ic-db-schema-owner-consumer'   => 'ic-db-schema-owner-producer',
);
my %packages;
for my $manifest ('Cargo.toml', 'qualification/Cargo.toml') {
    open my $metadata, '-|', 'cargo', 'metadata', '--format-version', '1',
        '--no-deps', '--locked', '--manifest-path', $manifest
        or die "cargo metadata: $!";
    my $workspace = decode_json(do { local $/; <$metadata> });
    close $metadata or die "cargo metadata failed for $manifest\n";
    $packages{$_->{name}} = $_ for @{$workspace->{packages}};
}

sub has_script {
    return scalar grep { grep { $_ eq 'custom-build' } @{$_->{kind}} } @{$_[0]->{targets}};
}

my %owners;
for my $package (values %packages) {
    for my $dep (@{$package->{dependencies}}) {
        next unless $dep->{name} eq 'ic-db' && ($dep->{kind} // '') eq 'build';
        next unless grep { $_ eq 'build' } @{$dep->{features}};
        $owners{$package->{name}} = 1;
        die "$package->{name}: unused IC DB generator dependency without a build script\n"
            unless has_script($package);
    }
}

for my $name (sort keys %consumer_of) {
    my $owner = $consumer_of{$name};
    my $consumer = $packages{$name} or die "Missing consumer fixture $name\n";
    die "$owner must generate its shared schema in its own build script\n" unless $owners{$owner};
    die "$name must import $owner as a normal dependency\n"
        unless grep { $_->{name} eq $owner && !defined $_->{kind} } @{$consumer->{dependencies}};
    die "$name is an import-only fixture; remove the consumer build script and import the typed store roster from $owner\n"
        if has_script($consumer);
}

for my $package (values %packages) {
    for my $dep (@{$package->{dependencies}}) {
        next unless $owners{$dep->{name}};
        die "$package->{name}: import $dep->{name} as a normal dependency, not a build dependency\n"
            if ($dep->{kind} // '') eq 'build';
    }
}
print "Shared schema build ownership verified in both Cargo workspaces.\n";

# The frozen comparison schema uses IcyDB's own prelude.
my $attribute = qr/(?:canister|store|entity|enum_|list|map|newtype|record|set)\b/;
find({ no_chdir => 1, wanted => sub {
    if (-d) {
        $File::Find::prune = 1 if m{(?:^|/)(?:target|ic-db-icydb-baseline-schema)$};
        return;
    }
    return unless /\.rs$/;
    open my $source, '<', $_ or die "$_: $!";
    my $rust = do { local $/; <$source> };
    die "$_: use unqualified IC DB schema attributes from the design prelude\n"
        if $rust =~ /^\h*(?:\/\/[!\/]\h*(?:\#\h+)?)?\#\[\s*(?:::)?ic_db::$attribute/m;
    return unless $rust =~ /^\h*\#\[\s*$attribute/m;
    die "$_: import ic_db::design::prelude::* when declaring schema types\n"
        unless $rust =~ /\buse\s+ic_db::(?:design::prelude::\*|\{[^;]*\bdesign::prelude::\*)/;
}}, 'crates', 'qualification');
