from dataclasses import dataclass
from datetime import datetime
from typing import Optional


@dataclass
class ReleaseContext:
    dry_run: bool = False
    component: Optional[str] = None
    reference_time: Optional[datetime] = None
    release_branch: Optional[str] = None


ctx = ReleaseContext()
