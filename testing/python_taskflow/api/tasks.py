from fastapi import APIRouter
from controllers.TaskController import TaskController

router = APIRouter()
controller = TaskController()

@router.get("/tasks/{task_id}")
def get_task(task_id: int):
    return controller.get_task(task_id)
