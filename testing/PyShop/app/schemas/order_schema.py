from pydantic import BaseModel

class OrderBase(BaseModel):
    total_amount: float

class OrderCreate(OrderBase):
    product_ids: list[int]

class OrderResponse(OrderBase):
    id: int
    user_id: int
    status: str

    class Config:
        from_attributes = True
