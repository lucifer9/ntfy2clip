#!/usr/bin/perl
use strict;
use warnings;
use JSON::PP;
binmode STDIN;
binmode STDOUT;
$| = 1;
open my $pids, '>>', "$0.pids" or die "pid journal";
print $pids "$$\n";
close $pids;
my $json = JSON::PP->new->utf8;
my $text = "fresh helper";
sub read_exact {
    my ($length) = @_;
    my $bytes = '';
    while (length($bytes) < $length) {
        my $n = read(STDIN, my $chunk, $length - length($bytes));
        exit 0 if defined($n) && $n == 0;
        die "read" unless defined $n;
        $bytes .= $chunk;
    }
    return $bytes;
}
while (1) {
    my $length = unpack('N', read_exact(4));
    my $request = $json->decode(read_exact($length));
    my $operation = $request->{operation};
    my $id = $request->{id};
    my $version = 1;
    my $result;
    if (!ref($operation) && $operation eq 'Capabilities') {
        $result = 'Ready';
    } elsif (!ref($operation) && $operation eq 'Read') {
        $result = { Snapshot => { value => { Text => $text }, revision => 0 } };
    } else {
        $text = $operation->{Write};
        exit 0 if $text eq 'fault:exit';
        sleep 60 if $text eq 'fault:timeout';
        if ($text eq 'fault:oversize') {
            print pack('N', 16 * 1024 * 1024 + 1);
            next;
        }
        $id++ if $text eq 'fault:id';
        $version = 2 if $text eq 'fault:version';
        $result = 'Written';
    }
    my $body = $json->encode({ version => $version, id => $id, result => $result });
    print pack('N', length($body)), $body;
}
