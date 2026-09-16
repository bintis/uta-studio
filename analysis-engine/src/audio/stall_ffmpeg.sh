#!/bin/sh
# Ignore ffmpeg arguments and wait to be killed. This file is never rewritten
# by tests so Linux execve cannot fail with ETXTBSY against a writable inode.
exec sleep 30
