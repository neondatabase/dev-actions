from dataclasses import dataclass, field
from datetime import datetime
from typing import Optional
from os import environ


@dataclass
class ReleaseContext:
    _dry_run: Optional[bool] = field(default=None, repr=False)
    _component: Optional[str] = field(default=None, repr=False)
    _reference_time: Optional[datetime] = field(default=None, repr=False)

    @property
    def dry_run(self) -> bool:
        if self._dry_run is None:
            raise RuntimeError("ctx.dry_run is not set!")
        return self._dry_run

    @dry_run.setter
    def dry_run(self, value: bool):
        if self._dry_run is not None and "PYTEST_CURRENT_TEST" not in environ:
            raise RuntimeError("ctx.dry_run is already set!")
        self._dry_run = value

    @property
    def component(self) -> str:
        if self._component is None:
            raise RuntimeError("ctx.component is not set!")
        return self._component

    @component.setter
    def component(self, value: str):
        if self._component is not None and "PYTEST_CURRENT_TEST" not in environ:
            raise RuntimeError("ctx.component is already set!")
        self._component = value

    @property
    def reference_time(self) -> datetime:
        if self._reference_time is None:
            raise RuntimeError("ctx.reference_time is not set!")
        return self._reference_time

    @reference_time.setter
    def reference_time(self, value: datetime):
        if self._reference_time is not None and "PYTEST_CURRENT_TEST" not in environ:
            raise RuntimeError("ctx.reference_time is already set!")
        self._reference_time = value

    def validate(self) -> None:
        if self._component is None:
            raise RuntimeError("ctx.component is not set")
        if self._reference_time is None:
            raise RuntimeError("ctx.reference_time is not set")
        if self._dry_run is None:
            raise RuntimeError("ctx.dry_run is not set")


ctx = ReleaseContext()
