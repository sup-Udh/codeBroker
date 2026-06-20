import { User, UserCreateDTO, UserUpdateDTO } from '../models/User';
import * as crypto from 'crypto';

export class UserRepository {
  private users: Map<string, User> = new Map();

  constructor() {
    // Seed an admin user
    this.create({
      email: 'admin@codebroker.com',
      passwordHash: 'admin-hash-mock',
      name: 'Admin User',
      role: 'admin'
    });
  }

  public async findAll(): Promise<User[]> {
    return Array.from(this.users.values());
  }

  public async findById(id: string): Promise<User | null> {
    return this.users.get(id) || null;
  }

  public async findByEmail(email: string): Promise<User | null> {
    for (const user of this.users.values()) {
      if (user.email === email) {
        return user;
      }
    }
    return null;
  }

  public async create(data: UserCreateDTO): Promise<User> {
    const id = crypto.randomUUID();
    const now = new Date();
    const newUser: User = {
      id,
      ...data,
      createdAt: now,
      updatedAt: now
    };
    this.users.set(id, newUser);
    return newUser;
  }

  public async update(id: string, data: UserUpdateDTO): Promise<User | null> {
    const existing = this.users.get(id);
    if (!existing) return null;

    const updated: User = {
      ...existing,
      ...data,
      updatedAt: new Date()
    };
    this.users.set(id, updated);
    return updated;
  }

  public async delete(id: string): Promise<boolean> {
    return this.users.delete(id);
  }
}
