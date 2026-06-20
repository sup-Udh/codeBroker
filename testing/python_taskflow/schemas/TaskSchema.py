from pydantic import BaseModel
from typing import Optional

class TaskCreate(BaseModel):
    title: str
    project_id: int
    assignee_id: Optional[int] = None

class TaskResponse(BaseModel):
    id: int
    title: str
    project_id: int
    assignee_id: Optional[int]
