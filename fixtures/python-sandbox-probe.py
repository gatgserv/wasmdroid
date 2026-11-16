import os
import errno
import socket
import sys

assert not os.path.exists("/system/build.prop"), "host filesystem leaked"
assert not os.path.exists("/data/user/0/dev.wasmdroid.app/files/host-only-sentinel")
with open("/tmp/wasmdroid-spike.txt", "w") as output:
    output.write("private guest data")
with open("/tmp/wasmdroid-spike.txt") as source:
    assert source.read() == "private guest data"

try:
    with socket.socket() as connection:
        connection.connect(("127.0.0.1", 9))
except OSError as error:
    assert error.errno in (errno.ENOSYS, errno.ENOTSUP, errno.EACCES, errno.EPERM), error
    print("WASMDROID_NETWORK_DENIED")
else:
    raise AssertionError("guest unexpectedly opened a host socket")

print("WASMDROID_PYTHON_OK")
print(sys.version)
