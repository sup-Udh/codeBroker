import { UserRepository } from '../repositories/UserRepository';
import { generateToken } from '../utils/jwt';
import * as crypto from 'crypto';

export class AuthService {
  private userRepository: UserRepository;

  constructor(userRepository: UserRepository) {
    this.userRepository = userRepository;
  }

  // Mock hash for simplicity
  private hashPassword(password: string): string {
    return crypto.createHash('sha256').update(password).digest('hex');
  }

  public async register(email: string, password: string, name: string): Promise<{ token: string, user: any }> {
    const existing = await this.userRepository.findByEmail(email);
    if (existing) {
      throw new Error('User already exists');
    }

    const passwordHash = this.hashPassword(password);
    const user = await this.userRepository.create({
      email,
      passwordHash,
      name,
      role: 'customer'
    });

    const token = generateToken({ userId: user.id, role: user.role });
    
    // Don't return password hash
    const { passwordHash: _, ...safeUser } = user;
    return { token, user: safeUser };
  }

  public async login(email: string, password: string): Promise<{ token: string, user: any }> {
    const user = await this.userRepository.findByEmail(email);
    if (!user) {
      throw new Error('Invalid credentials');
    }

    const passwordHash = this.hashPassword(password);
    if (user.passwordHash !== passwordHash) {
      throw new Error('Invalid credentials');
    }

    const token = generateToken({ userId: user.id, role: user.role });
    const { passwordHash: _, ...safeUser } = user;
    
    return { token, user: safeUser };
  }
}
