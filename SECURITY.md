# Security Policy

## Supported Versions

We release patches for security vulnerabilities. Which versions are eligible for receiving such patches depends on the CVSS v3.0 Rating:

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

If you discover a security vulnerability within Bayesian SSH, please send an email to [your-email@example.com]. All security vulnerabilities will be promptly addressed.

**Please do not report security vulnerabilities through public GitHub issues.**

## Security Best Practices

### For Users
- Keep your SSH keys secure and use strong passphrases
- Regularly update your SSH client and server software
- Use key-based authentication instead of passwords when possible
- Monitor your SSH connections for suspicious activity

### For Developers
- Follow Rust security best practices
- Keep dependencies updated
- Use `cargo audit` to check for known vulnerabilities
- Review all code changes for security implications

## Disclosure Policy

When we receive a security bug report, we will:

1. Confirm the problem and determine affected versions
2. Audit code to find any similar problems
3. Prepare fixes for all supported versions
4. Release a new version with the fix
5. Publicly announce the vulnerability

## Security Updates

Security updates will be released as patch versions (e.g., 0.1.1, 0.1.2) and will be clearly marked in the release notes.

## Contact

- **Security Email**: [your-email@example.com]
- **PGP Key**: [your-pgp-key-fingerprint]
- **GitHub Security Advisories**: [repository-security-tab]

## Acknowledgments

We appreciate security researchers who responsibly disclose vulnerabilities. Contributors will be acknowledged in our security advisories.
