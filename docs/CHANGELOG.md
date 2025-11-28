# Changelog

All notable changes to this project will be documented in this file.

## [2025-11-28 14:30] - Optimize PR Workflow Test Job

### Changed
- `.github/workflows/pr.yaml`: Added artifact upload/download to reuse build artifacts from build job in test job

### Why
The test job was rebuilding the entire project despite running after the build job, wasting CI time and resources. By uploading build artifacts (target directory) from the build job and downloading them in the test job, we eliminate redundant compilation.

### Impact
- [ ] Breaking change
- [ ] Requires cluster rollout
- [x] Config change only (CI/CD workflow)
- [ ] Documentation only

**Expected Benefit:** Significantly reduced PR CI time by avoiding duplicate builds in the test job.
