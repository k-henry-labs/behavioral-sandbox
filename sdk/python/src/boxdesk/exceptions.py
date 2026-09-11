"""Exception types raised by the Boxdesk SDK.

The dividing line these types encode is the one the CLI draws: a guest command
that exits non-zero is *not* an error, it is a :class:`~boxdesk.models.Run`
with a non-zero ``end_code``.  Only a failure of Boxdesk itself -- a missing
binary, an absent guest root, a hypervisor that does not answer -- reaches
here.
"""

from typing import Optional


class BoxdeskException(Exception):
    """Base class for every exception this package raises."""


class BoxdeskNotFound(BoxdeskException, FileNotFoundError):
    """A path the sandbox needs is not there -- most often the guest root.

    Also a :class:`FileNotFoundError`, so callers written against the earlier
    subprocess SDK, where this meant a missing binary, keep working.
    """


class BoxdeskError(BoxdeskException, RuntimeError):
    """The sandbox itself failed: no hypervisor, a posture that cannot be met,
    a run store that cannot be opened.

    ``stderr`` is the CLI's own message, passed through verbatim: it is written
    for a person and maintained upstream, so this package never rewords it.
    ``exit_code`` is the process exit status, which for a successful run would
    have been the *guest command's* status and so carries no meaning of its own.
    """

    def __init__(
        self, stderr: str, exit_code: int, detail: Optional[str] = None
    ) -> None:
        self.stderr = stderr
        self.exit_code = exit_code
        # Boxdesk's own words win whenever it wrote any; `detail` only fills
        # the gap when it exited silently.
        silent = (
            f"boxdesk exited with status {exit_code} without printing a JSON document"
        )
        super().__init__(stderr.strip() or detail or silent)
