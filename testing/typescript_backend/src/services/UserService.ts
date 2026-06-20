import { UserRepository } from '../repositories/UserRepository';
import { User, UserUpdateDTO } from '../models/User';

export class UserService {
  private userRepository: UserRepository;

  constructor(userRepository: UserRepository) {
    this.userRepository = userRepository;
  }

  public async getAllUsers(): Promise<User[]> {
    return this.userRepository.findAll();
  }

  public async getUserById(id: string): Promise<User> {
    const user = await this.userRepository.findById(id);
    if (!user) {
      throw new Error('User not found');
    }
    return user;
  }

  public async updateUser(id: string, data: UserUpdateDTO): Promise<User> {
    // Ensure user exists
    await this.getUserById(id);
    
    const updated = await this.userRepository.update(id, data);
    if (!updated) {
      throw new Error('Failed to update user');
    }
    return updated;
  }

  public async deleteUser(id: string): Promise<void> {
    const deleted = await this.userRepository.delete(id);
    if (!deleted) {
      throw new Error('User not found or could not be deleted');
    }
  }
}
