package dev.wasmdroid.app.engine

internal object WorkerProtocol {
    const val START = 1
    const val STOP = 2
    const val TERMINATE = 3
    const val PING = 4
    const val COMPLETE_ACK = 5
    const val STDIN = 10
    const val STDIN_EOF = 11
    const val RESIZE = 12
    const val INTERRUPT = 13
    const val TERMINAL_FRAME = 14
    const val STATS = 15
    const val NETWORK = 16
    const val LAN_ENABLE = 17
    const val LAN_DISABLE = 18
    const val LAN_STATUS = 19
    const val LAN_REVOKE = 20
    const val LAN_SCOPE = "lan_scope"
    const val LAN_NETWORK = "lan_network"
    const val NETWORK_CONFIG = "network_config"
    const val GRACE_SECONDS = "grace_seconds"
    const val QUERY = "query"
    const val READY = 101
    const val COMPLETED = 102
    const val FAILED = 103
    const val PONG = 104
    const val CONTROL_DONE = 110
    const val CONTROL_ERROR = 111
    const val REQUEST = "request"
    const val BYTES = "bytes"
    const val COLS = "cols"
    const val ROWS = "rows"
    const val WIDTH = "width"
    const val HEIGHT = "height"
    const val GENERATION = "generation"
    const val EGRESS_REQUIRED = "egress_required"
    const val SPEC = "spec"
    const val INSTANCE = "instance"
    const val OWNER = "owner"
    const val RESULT = "result"
    const val PID = "pid"
    const val UID = "uid"
    const val STOP_BEFORE_START = "stop_before_start"
    const val STOP_RECEIVED = "stop_received"
}
