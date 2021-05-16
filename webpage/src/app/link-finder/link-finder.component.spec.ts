import { ComponentFixture, TestBed } from '@angular/core/testing';

import { LinkFinderComponent } from './link-finder.component';

describe('LinkFinderComponent', () => {
  let component: LinkFinderComponent;
  let fixture: ComponentFixture<LinkFinderComponent>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      declarations: [ LinkFinderComponent ]
    })
    .compileComponents();
  });

  beforeEach(() => {
    fixture = TestBed.createComponent(LinkFinderComponent);
    component = fixture.componentInstance;
    fixture.detectChanges();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
