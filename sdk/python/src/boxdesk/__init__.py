"""The Python SDK for Boxdesk.

Boxdesk runs untrusted code inside a hardware-isolated virtual machine on your
own machine.  This package spawns nothing and parses nothing: it calls the same
``execute_sandbox`` the ``boxdesk`` binary calls, in this process, and the
:class:`Run` it hands back is built in Rust from the run record itself.

    >>> import boxdesk
    >>> run = boxdesk.Boxdesk().run(["python3", "-c", "print(6*7)"])
    >>> run.stdout
    '42\\n'
"""

from .client import Sandbox, Boxdesk
from .exceptions import BoxdeskError, BoxdeskException, BoxdeskNotFound
from .models import File, Posture, Run
from .options import Pairs, RunOptions

__version__ = "0.1.0"

__all__ = [
    "Boxdesk",
    "Sandbox",
    "Run",
    "RunOptions",
    "Pairs",
    "Posture",
    "File",
    "BoxdeskException",
    "BoxdeskError",
    "BoxdeskNotFound",
    "__version__",
]
