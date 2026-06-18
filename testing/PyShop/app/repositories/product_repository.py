from sqlalchemy.orm import Session
from app.models.product import Product

class ProductRepository:
    def __init__(self, db: Session):
        self.db = db

    def get_product(self, product_id: int):
        return self.db.query(Product).filter(Product.id == product_id).first()
        
    def get_all(self):
        return self.db.query(Product).all()

    def create_product(self, name: str, description: str, price: float):
        product = Product(name=name, description=description, price=price)
        self.db.add(product)
        self.db.commit()
        self.db.refresh(product)
        return product
