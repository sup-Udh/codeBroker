from sqlalchemy import Column, Integer, String, DateTime
from datetime import datetime
from app.config.database import Base

class AuditLog(Base):
    __tablename__ = "audit_logs"
    id = Column(Integer, primary_key=True, index=True)
    action = Column(String)
    resource = Column(String)
    user_id = Column(Integer)
    timestamp = Column(DateTime, default=datetime.utcnow)
