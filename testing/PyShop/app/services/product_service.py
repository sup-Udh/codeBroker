from sqlalchemy.orm import Session
from app.repositories.product_repository import ProductRepository
from app.repositories.audit_repository import AuditRepository
from app.schemas.product_schema import ProductCreate
from app.services.inventory_service import InventoryService

class ProductService:
    def __init__(self, db: Session):
        self.product_repo = ProductRepository(db)
        self.audit_repo = AuditRepository(db)
        self.inventory_service = InventoryService(db)

    def create_product(self, data: ProductCreate, admin_id: int):
        product = self.product_repo.create_product(data.name, data.description, data.price)
        self.audit_repo.log_action("CREATE_PRODUCT", f"Product_{product.id}", admin_id)
        return product
