from sqlalchemy.orm import Session
from app.repositories.audit_repository import AuditRepository

def cleanup_old_logs(db: Session):
    repo = AuditRepository(db)
    print("Cleaning up logs...")
