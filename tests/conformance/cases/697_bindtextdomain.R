# The unset-domain default locale directory is deployment metadata (""
# on this reference, an absolute path on Linux). The defined
# cross-platform property is its shape: empty or an absolute path.
cat(grepl("^$|^/", bindtextdomain("rport-test-domain-xyz")), "\n", sep = "")
cat(bindtextdomain("rport-test-domain-xyz", "/tmp"), "\n", sep = "")
cat(bindtextdomain("rport-test-domain-xyz"), "\n", sep = "")
cat(bindtextdomain(NULL, NULL), "\n", sep = "")
