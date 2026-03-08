from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class Config:
    project_dir: str
    output_dir: str = ""
    provider: str = "anthropic"
    model: str = "claude-haiku-4-5-20251001"
    include_patterns: list[str] = field(default_factory=list)
    exclude_patterns: list[str] = field(default_factory=lambda: [
        "*_test.go", "vendor/*", "*.pb.go", "*_generated.*", "*/target/*",
    ])
    sql_include_patterns: list[str] = field(default_factory=lambda: ["**/*.sql"])
    dry_run: bool = False
    resume: bool = False
    concurrency: int = 4

    def __post_init__(self) -> None:
        if not self.output_dir:
            import os
            project_name = os.path.basename(self.project_dir.rstrip("/"))
            self.output_dir = f"literate-code-{project_name}"
