export interface User {
  id: string;
  email: string;
  passwordHash: string;
  name: string;
  role: 'admin' | 'customer';
  createdAt: Date;
  updatedAt: Date;
}

export type UserCreateDTO = Omit<User, 'id' | 'createdAt' | 'updatedAt'>;
export type UserUpdateDTO = Partial<Omit<User, 'id' | 'createdAt' | 'updatedAt'>>;
