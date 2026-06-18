from sqlalchemy.orm import Session
from app.models.audit_log import AuditLog
from app.utils.audit_utils import format_audit_message

class AuditRepository:
    def __init__(self, db: Session):
        self.db = db

    def log_action(self, action: str, resource: str, user_id: int):
        message = format_audit_message(action, resource, user_id)
        log = AuditLog(action=action, resource=resource, user_id=user_id)
        self.db.add(log)
        self.db.commit()
        return log
